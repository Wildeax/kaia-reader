--[[--
View module that paints Kaia stickers on the current page and records where
they were drawn, so taps can be matched to them.

It is registered with ReaderView:registerViewModule. Stickers are positioned
from their anchors at paint time, so a font or margin change moves them with
the text and nothing is stored.

Images are decoded per asset and pixel size, and freed by free() when the
book closes.

@module koplugin.KaiaStickers.layer
--]]--

local Blitbuffer = require("ffi/blitbuffer")
local Geom = require("ui/geometry")
local RenderImage = require("ui/renderimage")
local Screen = require("device").screen
local logger = require("logger")

local MAX_CACHED_IMAGES = 32

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

--- Frees every decoded image. Called when the book closes.
function Layer:free()
    for key, img in pairs(self.images) do
        img:free()
        self.images[key] = nil
    end
    self.image_count = 0
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

-- Returns the decoded image for an asset at a pixel size, or nil.
function Layer:image(store, sha256, w, h)
    local key = sha256 .. ":" .. w .. "x" .. h
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
    img = RenderImage:renderImageFile(path, false, w, h)
    if not img then
        logger.warn("kaia stickers: cannot decode asset", sha256)
        return nil
    end
    if self.image_count >= MAX_CACHED_IMAGES then
        self:free()
    end
    self.images[key] = img
    self.image_count = self.image_count + 1
    return img
end

-- Screen rectangle of a reflowable sticker, or nil when its anchor is not on
-- this screen. The second value is "lost" when the locator no longer resolves.
local function reflowableRect(doc, sticker, w, h)
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
    -- dy is in units of the line height. dy = -1 puts the bottom of the sticker
    -- on the top of the line, and the default -1.1 leaves a small gap above it.
    local bottom = box.y + box.h + anchor.offset.dy * box.h
    return {
        x = box.x + anchor.offset.dx * box.h,
        y = bottom - h,
        w = w,
        h = h,
    }
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
            local w = math.floor(sticker.size.w * screen_w + 0.5)
            local h = math.floor(sticker.size.h * screen_h + 0.5)
            local rect
            if sticker.anchor.type == "reflowable" and not paging then
                local lost
                rect, lost = reflowableRect(self.ui.document, sticker, w, h)
                if lost then
                    -- The locator is gone. plugin:repairSticker searches the
                    -- book for the quote and either moves the sticker or orphans
                    -- it. A moved sticker is placed again in this same paint.
                    if plugin:repairSticker(sticker) then
                        rect = reflowableRect(self.ui.document, sticker, w, h)
                    else
                        rect = nil
                    end
                end
            elseif sticker.anchor.type == "fixed" and paging and pages[sticker.anchor.page] then
                rect = self:fixedRect(sticker, w, h)
            end
            if rect and w > 0 and h > 0 then
                local img = self:image(store, sticker.asset, w, h)
                if img then
                    blit(bb, img, rect)
                    self.hits[#self.hits + 1] = { id = sticker.id, x = rect.x, y = rect.y, w = rect.w, h = rect.h }
                end
            end
        end
    end
end

return Layer
