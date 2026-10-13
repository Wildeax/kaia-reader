--[[--
View module that paints Kaia stickers on the current page and records where
they were drawn, so taps can be matched to them.

It is registered with ReaderView:registerViewModule. Stickers are positioned
from their anchors at paint time, so a font or margin change moves them with
the text and nothing is stored.

Reflowable stickers have two placements:

- margin: the image sits in the page margin beside the anchor line, on the
  right for left-to-right layouts and on the left for right-to-left ones. It is
  centred on the first line box, then moved by offset.dy line heights. When the
  margin is too narrow, the image is drawn over the end of the anchor line
  instead, faded so the text stays readable.
- inline: the image is drawn over the text, above the anchor line, at the
  offset given by dx and dy.

Images are decoded per asset and pixel size, and freed by free() when the
book closes.

@module koplugin.KaiaStickers.layer
--]]--

local Blitbuffer = require("ffi/blitbuffer")
local BD = require("ui/bidi")
local Geom = require("ui/geometry")
local RenderImage = require("ui/renderimage")
local Screen = require("device").screen
local Theme = require("kaia/theme")
local logger = require("logger")

local MAX_CACHED_IMAGES = 32
-- Gap between the margin sticker and the text column, and the margin side kept clear.
local MARGIN_GAP = Theme.space.xs
-- Below this margin width the sticker is drawn over the text instead.
local MIN_MARGIN_WIDTH = Screen:scaleBySize(22)
-- Opacity kept by the faded image used when the margin is too narrow.
local FADE_FACTOR = 0.4

local Layer = {}

--- Creates the layer for a plugin instance. The plugin owns the store.
function Layer:new(plugin)
    return setmetatable({
        plugin = plugin,
        images = {},
        image_count = 0,
        paths = {},
        hits = {},
    }, { __index = self })
end

-- Frees the decoded images. The hit list is kept, because a cache flush can
-- happen in the middle of a paint.
function Layer:freeImages()
    for key, img in pairs(self.images) do
        img:free()
        self.images[key] = nil
    end
    self.image_count = 0
end

--- Frees every decoded image and forgets the hits. Called when the book closes.
function Layer:free()
    self:freeImages()
    self.paths = {}
    self.hits = {}
end

--- Returns the sticker id under a screen position, or nil.
-- @tparam Geom pos screen position of a tap
function Layer:hitTest(pos)
    for i = #self.hits, 1, -1 do
        local hit = self.hits[i]
        if pos.x >= hit.x and pos.x < hit.x + hit.w and pos.y >= hit.y and pos.y < hit.y + hit.h then
            return hit.id
        end
    end
    return nil
end

-- Returns a copy of a premultiplied-alpha image with every channel and the alpha
-- scaled by FADE_FACTOR. Images without an alpha channel return nil.
local function fadedCopy(img)
    local bbtype = img:getType()
    local has_alpha = bbtype == Blitbuffer.TYPE_BB8A or bbtype == Blitbuffer.TYPE_BBRGB32
    if not has_alpha then
        return nil
    end
    local w, h = img:getWidth(), img:getHeight()
    local out = Blitbuffer.new(w, h, bbtype)
    for y = 0, h - 1 do
        for x = 0, w - 1 do
            local c = img:getPixel(x, y)
            if bbtype == Blitbuffer.TYPE_BB8A then
                out:setPixel(x, y, Blitbuffer.Color8A(
                    math.floor(c.a * FADE_FACTOR), math.floor(c.alpha * FADE_FACTOR)))
            else
                out:setPixel(x, y, Blitbuffer.ColorRGB32(
                    math.floor(c.r * FADE_FACTOR), math.floor(c.g * FADE_FACTOR),
                    math.floor(c.b * FADE_FACTOR), math.floor(c.alpha * FADE_FACTOR)))
            end
        end
    end
    return out
end

-- Returns the decoded image for an asset at a pixel size, or nil. When `faded`
-- is true, the faded copy is returned instead, or the plain image when it has
-- no alpha channel.
function Layer:image(store, sha256, w, h, faded)
    local key = sha256 .. ":" .. w .. "x" .. h .. (faded and ":faded" or "")
    local img = self.images[key]
    if img then
        return img
    end
    local path = self.paths[sha256]
    if not path then
        path = store:assetPath(sha256)
        if not path then
            return nil
        end
        self.paths[sha256] = path
    end
    if faded then
        local plain = self:image(store, sha256, w, h, false)
        if not plain then
            return nil
        end
        img = fadedCopy(plain)
        if not img then
            -- No alpha channel to fade. The plain image is cached under its own
            -- key only, so it is never freed twice.
            return plain
        end
    else
        img = RenderImage:renderImageFile(path, false, w, h)
    end
    if not img then
        logger.warn("kaia stickers: cannot decode asset", sha256)
        return nil
    end
    if self.image_count >= MAX_CACHED_IMAGES then
        self:freeImages()
    end
    self.images[key] = img
    self.image_count = self.image_count + 1
    return img
end

-- Left and right edges of the text column on screen, measured from the words
-- the engine lays out on this screen. The page margin values are not used when
-- the words can be measured, because they do not match the drawn column
-- exactly. Cached until the next paint.
function Layer:textColumn()
    if self.column then
        return self.column[1], self.column[2]
    end
    local screen_w, screen_h = Screen:getWidth(), Screen:getHeight()
    local left, right
    local range = self.ui.document:getTextFromPositions({ x = 0, y = 0 }, { x = screen_w, y = screen_h }, true)
    for _, box in ipairs(range and range.sboxes or {}) do
        left = math.min(left or box.x, box.x)
        right = math.max(right or box.x + box.w, box.x + box.w)
    end
    if not left then
        local margins = self.ui.document:getPageMargins() or {}
        left = margins.left or 0
        right = screen_w - (margins.right or 0)
    end
    self.column = { left, right }
    return left, right
end

-- Returns the placement of a reflowable anchor. Missing or unknown values read
-- as margin, the same default as the store.
local function placementOf(anchor)
    if anchor.placement == "inline" then
        return "inline"
    end
    return "margin"
end

-- Screen rectangle of a reflowable sticker, or nil when its anchor is not on
-- this screen. The second value is "lost" when the locator no longer resolves.
-- The third value is true when the image should be drawn faded over the text.
function Layer:reflowableRect(sticker, screen_w, screen_h)
    local doc = self.ui.document
    local anchor = sticker.anchor
    local word_end = doc:getNextVisibleWordEnd(anchor.locator)
    if not word_end then
        return nil, "lost"
    end
    local boxes = doc:getScreenBoxesFromPositions(anchor.locator, word_end, true)
    local box = boxes and boxes[1]
    if not box or box.h <= 0 then
        return nil
    end
    local dy = anchor.offset.dy
    local full_w = sticker.size.w * screen_w
    local full_h = sticker.size.h * screen_h

    if placementOf(anchor) == "inline" then
        -- dy is in units of the line height. dy = -1 puts the bottom of the sticker
        -- on the top of the line, and the default -1.1 leaves a small gap above it.
        local bottom = box.y + box.h + dy * box.h
        return {
            x = box.x + anchor.offset.dx * box.h,
            y = bottom - full_h,
            w = full_w,
            h = full_h,
        }, nil, false
    end

    local text_left, text_right = self:textColumn()
    local rtl = BD.mirroredUILayout()
    local room = rtl and text_left or (screen_w - text_right)
    local avail = room - 2 * MARGIN_GAP
    if avail < MIN_MARGIN_WIDTH then
        -- Too narrow for the margin: draw over the end of the anchor line, faded.
        local w, h = full_w, full_h
        return {
            x = math.max(0, box.x + box.w - w),
            y = box.y + box.h / 2 - h / 2 + dy * box.h,
            w = w,
            h = h,
        }, nil, true
    end
    local w = math.min(full_w, avail)
    local h = full_w > 0 and w * full_h / full_w or 0
    local x = rtl and (text_left - MARGIN_GAP - w) or (text_right + MARGIN_GAP)
    return {
        x = x,
        y = box.y + box.h / 2 - h / 2 + dy * box.h,
        w = w,
        h = h,
    }, nil, false
end

-- Screen rectangle of a fixed sticker, or nil when its page is not shown.
-- The sticker sits with its bottom edge on the anchor point.
function Layer:fixedRect(sticker, w, h)
    local anchor = sticker.anchor
    local dims = self.ui.document:getNativePageDimensions(anchor.page)
    if not dims then
        return nil
    end
    local point = self.view:pageToScreenTransform(anchor.page, Geom:new{
        x = anchor.x * dims.w,
        y = anchor.y * dims.h,
        w = 1,
        h = 1,
    })
    if not point then
        return nil
    end
    return { x = point.x, y = point.y - h, w = w, h = h }
end

-- Draws one decoded image with its alpha channel.
local function blit(bb, img, rect)
    local bbtype = img:getType()
    if bbtype == Blitbuffer.TYPE_BB8A or bbtype == Blitbuffer.TYPE_BBRGB32 then
        bb:pmulalphablitFrom(img, rect.x, rect.y, 0, 0, img:getWidth(), img:getHeight())
    else
        bb:blitFrom(img, rect.x, rect.y, 0, 0, img:getWidth(), img:getHeight())
    end
end

--- Paints the stickers of the current page. Called by ReaderView:paintTo.
function Layer:paintTo(bb, _x, _y)
    self.hits = {}
    self.column = nil
    local plugin = self.plugin
    if not plugin.store or not plugin:isVisible() then
        return
    end
    local ok, err = pcall(self.paintStickers, self, bb)
    if not ok then
        logger.warn("kaia stickers: paint failed", err)
    end
end

function Layer:paintStickers(bb)
    local plugin = self.plugin
    local store = plugin.store
    local paging = self.ui.paging
    local pages
    if paging then
        pages = {}
        for _, page in ipairs(self.view:getCurrentPageList()) do
            pages[page] = true
        end
    end
    local screen_w, screen_h = Screen:getWidth(), Screen:getHeight()
    for _, sticker in ipairs(plugin:stickers()) do
        -- Text-only stickers have no image, and are not drawn in version 1.
        if sticker.state == "attached" and sticker.size and sticker.asset then
            local rect, faded
            if sticker.anchor.type == "reflowable" and not paging then
                local lost
                rect, lost, faded = self:reflowableRect(sticker, screen_w, screen_h)
                if lost then
                    -- The locator is gone. plugin:repairSticker searches the
                    -- book for the quote and either moves the sticker or orphans
                    -- it. A moved sticker is placed again in this same paint.
                    if plugin:repairSticker(sticker) then
                        rect, _, faded = self:reflowableRect(sticker, screen_w, screen_h)
                    else
                        rect = nil
                    end
                end
            elseif sticker.anchor.type == "fixed" and paging and pages[sticker.anchor.page] then
                local w = sticker.size.w * screen_w
                local h = sticker.size.h * screen_h
                rect = self:fixedRect(sticker, w, h)
            end
            if rect then
                local w = math.floor(rect.w + 0.5)
                local h = math.floor(rect.h + 0.5)
                if w > 0 and h > 0 then
                    local img = self:image(store, sticker.asset, w, h, faded)
                    if img then
                        rect = { x = math.floor(rect.x + 0.5), y = math.floor(rect.y + 0.5), w = w, h = h }
                        blit(bb, img, rect)
                        self.hits[#self.hits + 1] = { id = sticker.id, x = rect.x, y = rect.y, w = w, h = h }
                    end
                end
            end
        end
    end
end

return Layer
