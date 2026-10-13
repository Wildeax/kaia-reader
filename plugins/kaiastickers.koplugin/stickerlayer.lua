--[[--
View module that paints Kaia stickers on the current page and records where
they were drawn, so taps can be matched to them.

It is registered with ReaderView:registerViewModule. Stickers are positioned
from their anchors at paint time, so a font or margin change moves them with
the text and nothing is stored.

Reflowable stickers have two placements:

- margin: the sticker sits in the page margin beside the anchor line, on the
  right for left-to-right layouts and on the left for right-to-left ones. It is
  centred on the first line box, then moved by offset.dy line heights. When the
  margin is too narrow, an image is drawn over the end of the anchor line
  instead, faded so the text stays readable.
- inline: the sticker is drawn over the text, above the anchor line, at the
  offset given by dx and dy.

Paged stickers (PDF, DjVu, CBZ) are anchored in native page coordinates: the
point is a fraction of the unrotated, uncropped page. The view maps native
coordinates to the screen with pageToScreenTransform, the same mapping KOReader
uses for its own highlights. The size is a fraction of the native page size, so
the drawn rectangle scales with the page at every zoom. The page is not rotated
by the view in this fork, so a rotation of the screen (setRotationMode) changes
the layout but not the mapping. Cropped views (ReaderView.use_bbox) use the same
native coordinates, so no crop correction is applied.

Text-only stickers (text set, no asset) never cover words. In a margin wide
enough for a box (NOTE_MIN_WIDTH), a text note is drawn as a rounded box with up
to three lines of caption text. In a narrower margin it is drawn as a marker: a
small rounded square with three lines, a note glyph, centred on the anchor line.
The full text opens when the marker is tapped. Inline placement does not apply to
text notes. A paged text note is a marker too, placed left of its line, or above
it when the page has no room on the left.

Paged stickers (PDF, DjVu, CBZ) are anchored in native page coordinates: the
point is a fraction of the unrotated, uncropped page. The view maps native
coordinates to the screen with pageToScreenTransform, the same mapping KOReader
uses for its own highlights. The size is a fraction of the native page size, so
the drawn rectangle scales with the page at every zoom. The page is not rotated
by the view in this fork, so a rotation of the screen (setRotationMode) changes
the layout but not the mapping. Cropped views (ReaderView.use_bbox) use the same
native coordinates, so no crop correction is applied.

Images are decoded per asset and pixel size, and freed by free() when the book
closes.

@module koplugin.KaiaStickers.layer
--]]--

local Blitbuffer = require("ffi/blitbuffer")
local BD = require("ui/bidi")
local FrameContainer = require("ui/widget/container/framecontainer")
local Geom = require("ui/geometry")
local RenderImage = require("ui/renderimage")
local Screen = require("device").screen
local TextBoxWidget = require("ui/widget/textboxwidget")
local Theme = require("kaia/theme")
local logger = require("logger")

local MAX_CACHED_IMAGES = 32
-- Gap between the margin sticker and the text column, and the margin side kept clear.
local MARGIN_GAP = Theme.space.xs
-- Below this margin width an image is drawn over the text instead.
local MIN_MARGIN_WIDTH = Screen:scaleBySize(22)
-- A text note needs more room than an image to stay readable. Narrower
-- margins draw it opaque over the end of its line.
local NOTE_MIN_WIDTH = Screen:scaleBySize(80)
-- Opacity kept by the faded image used when the margin is too narrow.
local FADE_FACTOR = 0.4
-- Text notes show at most this many lines of caption text.
local NOTE_MAX_LINES = 3
-- Line spacing in em, the same value TextBoxWidget uses by default.
local NOTE_LINE_SPACING = 0.3
-- A text note marker is a square as wide as the margin allows, within these bounds.
local MARKER_MIN = Screen:scaleBySize(16)
local MARKER_MAX = Screen:scaleBySize(28)

local function clampRange(value, low, high)
    return math.max(low, math.min(high, value))
end

local Layer = {}

--- Creates the layer for a plugin instance. The plugin owns the store.
function Layer:new(plugin)
    return setmetatable({
        plugin = plugin,
        images = {},
        image_count = 0,
        notes = {},
        note_count = 0,
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

-- Frees the built text notes.
function Layer:freeNotes()
    for key, note in pairs(self.notes) do
        note.frame:free()
        self.notes[key] = nil
    end
    self.note_count = 0
end

--- Frees every decoded image and note, and forgets the hits. Called when the book closes.
function Layer:free()
    self:freeImages()
    self:freeNotes()
    self.paths = {}
    self.hits = {}
end

--- Returns the sticker id under a screen position, and "marker" for a text
-- note marker or "sticker" for anything else. Returns nil when nothing is there.
-- @tparam Geom pos screen position of a tap
function Layer:hitTest(pos)
    -- Small stickers and note markers get a larger tap area, centred on the
    -- drawn rectangle. It stays under the full touch target so taps in the
    -- margin still turn pages.
    local min_side = math.floor(Theme.touch_target * 0.6)
    for i = #self.hits, 1, -1 do
        local hit = self.hits[i]
        local w, h = math.max(hit.w, min_side), math.max(hit.h, min_side)
        local x = hit.x - math.floor((w - hit.w) / 2)
        local y = hit.y - math.floor((h - hit.h) / 2)
        if pos.x >= x and pos.x < x + w and pos.y >= y and pos.y < y + h then
            return hit.id, hit.kind
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

-- Returns the frame of a text note drawn `w` pixels wide, and its height, or
-- nil when the width is too small for text. Notes are cached by id, width and text.
function Layer:note(sticker, w)
    w = math.floor(w)
    if w <= 0 then
        return nil
    end
    local key = sticker.id .. ":" .. w .. ":" .. sticker.text
    local cached = self.notes[key]
    if cached then
        return cached.frame, cached.h
    end
    local face = Theme:font("caption")
    local padding = Theme.space.xs
    local border = Theme.line.divider
    local inner_w = w - 2 * (padding + border)
    if inner_w < 1 then
        return nil
    end
    local line_h = math.floor((1 + NOTE_LINE_SPACING) * face.size + 0.5)
    local text = TextBoxWidget:new{
        text = sticker.text,
        face = face,
        fgcolor = Theme.color.ink,
        -- The text box fills its own background, so it takes the note colour.
        bgcolor = Theme.color.surface_alt,
        width = inner_w,
        height = line_h * NOTE_MAX_LINES,
        height_adjust = true,
        height_overflow_show_ellipsis = true,
    }
    local frame = FrameContainer:new{
        background = Theme.color.surface_alt,
        color = Theme.color.divider,
        bordersize = border,
        radius = Theme.radius.button,
        padding = padding,
        margin = 0,
        text,
    }
    local h = frame:getSize().h
    if self.note_count >= MAX_CACHED_IMAGES then
        self:freeNotes()
    end
    self.notes[key] = { frame = frame, h = h }
    self.note_count = self.note_count + 1
    return frame, h
end

-- Returns a function that gives the drawn height of a sticker at a width. An
-- image keeps its shape. A text note takes the height of its text.
function Layer:heightFunc(sticker, full_w, full_h)
    if sticker.asset then
        return function(w)
            return full_w > 0 and w * full_h / full_w or 0
        end
    end
    return function(w)
        local _, h = self:note(sticker, w)
        return h or 0
    end
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
-- The fourth value is true when a text note is drawn as a marker.
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
    local height_for = self:heightFunc(sticker, full_w, full_h)
    local text_left, text_right = self:textColumn()
    local rtl = BD.mirroredUILayout()
    local room = rtl and text_left or (screen_w - text_right)
    local avail = room - 2 * MARGIN_GAP

    if sticker.asset and placementOf(anchor) == "inline" then
        -- dy is in units of the line height. dy = -1 puts the bottom of the sticker
        -- on the top of the line, and the default -1.1 leaves a small gap above it.
        local h = height_for(full_w)
        local bottom = box.y + box.h + dy * box.h
        return {
            x = box.x + anchor.offset.dx * box.h,
            y = bottom - h,
            w = full_w,
            h = h,
        }, nil, false, false
    end

    if not sticker.asset then
        if avail < NOTE_MIN_WIDTH then
            -- A text note never covers words. Without room for its box, a marker
            -- stands in the margin, centred on the anchor line.
            local size = clampRange(avail, MARKER_MIN, MARKER_MAX)
            local x = rtl and (text_left - MARGIN_GAP - size) or (text_right + MARGIN_GAP)
            return {
                x = clampRange(x, 0, screen_w - size),
                y = box.y + box.h / 2 - size / 2 + dy * box.h,
                w = size,
                h = size,
            }, nil, false, true
        end
    elseif avail < MIN_MARGIN_WIDTH then
        -- Too narrow for the margin: an image is drawn over the end of its line,
        -- faded so the text stays readable.
        local h = height_for(full_w)
        return {
            x = math.max(0, box.x + box.w - full_w),
            y = box.y + box.h / 2 - h / 2 + dy * box.h,
            w = full_w,
            h = h,
        }, nil, true, false
    end

    local w = math.min(full_w, avail)
    local h = height_for(w)
    local x = rtl and (text_left - MARGIN_GAP - w) or (text_right + MARGIN_GAP)
    return {
        x = x,
        y = box.y + box.h / 2 - h / 2 + dy * box.h,
        w = w,
        h = h,
    }, nil, false, false
end

-- Zoom of a page on the screen, or nil when the page is not shown.
function Layer:pageZoom(page)
    local view = self.view
    if view.page_scroll then
        for _, state in ipairs(view.page_states or {}) do
            if state.page == page then
                return state.zoom
            end
        end
        return nil
    end
    if view.state.page == page then
        return view.state.zoom
    end
    return nil
end

-- Screen rectangle of a fixed sticker, or nil when its page is not shown. The
-- second value is true for a text note marker. An image sits with its bottom
-- edge on the anchor point and its width and height are fractions of the page
-- size on the screen. A text note marker sits left of the anchor point, or
-- above it when there is no room on the left.
function Layer:fixedRect(sticker)
    local anchor = sticker.anchor
    local dims = self.ui.document:getNativePageDimensions(anchor.page)
    local zoom = dims and self:pageZoom(anchor.page)
    if not zoom then
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
    if not sticker.asset then
        local size = MARKER_MAX
        local x, y = point.x - MARGIN_GAP - size, point.y
        if x < 0 then
            x, y = point.x, point.y - size - MARGIN_GAP
        end
        return {
            x = clampRange(x, 0, Screen:getWidth() - size),
            y = math.max(0, y),
            w = size,
            h = size,
        }, true
    end
    local full_w = sticker.size.w * dims.w * zoom
    local full_h = sticker.size.h * dims.h * zoom
    local h = self:heightFunc(sticker, full_w, full_h)(full_w)
    return { x = point.x, y = point.y - h, w = full_w, h = h }, false
end

-- Paints a text note marker: a rounded square with three short lines, the
-- glyph of a note. The square is as wide as it is tall.
local function paintMarker(bb, rect)
    local radius = math.floor(rect.w / 4)
    bb:paintRoundedRect(rect.x, rect.y, rect.w, rect.h, Theme.color.surface_alt, radius)
    bb:paintBorder(rect.x, rect.y, rect.w, rect.h, Theme.line.divider, Theme.color.divider, radius)
    local pad = math.floor(rect.w / 4)
    local line_h = math.max(1, math.floor(rect.w / 12))
    local inner_w = rect.w - 2 * pad
    local gap = math.floor((rect.h - 2 * pad - 3 * line_h) / 2)
    for i = 0, 2 do
        local line_w = i == 2 and math.floor(inner_w * 0.6) or inner_w
        bb:paintRect(rect.x + pad, rect.y + pad + i * (line_h + gap), line_w, line_h, Theme.color.ink_secondary)
    end
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

-- Draws one sticker in its rectangle. Returns false when nothing was drawn.
function Layer:drawSticker(bb, sticker, rect, faded, marker)
    if marker then
        paintMarker(bb, rect)
        return true
    end
    if sticker.asset then
        local img = self:image(self.plugin.store, sticker.asset, rect.w, rect.h, faded)
        if not img then
            return false
        end
        blit(bb, img, rect)
        return true
    end
    local frame = self:note(sticker, rect.w)
    if not frame then
        return false
    end
    frame:paintTo(bb, rect.x, rect.y)
    return true
end

function Layer:paintStickers(bb)
    local plugin = self.plugin
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
        -- A sticker has an image, a text, or both. Only attached stickers are drawn.
        if sticker.state == "attached" and sticker.size and (sticker.asset or sticker.text) then
            local rect, faded, marker
            if sticker.anchor.type == "reflowable" and not paging then
                local lost
                rect, lost, faded, marker = self:reflowableRect(sticker, screen_w, screen_h)
                if lost then
                    -- The locator is gone. plugin:repairSticker searches the
                    -- book for the quote and either moves the sticker or orphans
                    -- it. A moved sticker is placed again in this same paint.
                    if plugin:repairSticker(sticker) then
                        rect, _, faded, marker = self:reflowableRect(sticker, screen_w, screen_h)
                    else
                        rect = nil
                    end
                end
            elseif sticker.anchor.type == "fixed" and paging and pages[sticker.anchor.page] then
                rect, marker = self:fixedRect(sticker)
            end
            if rect then
                local w = math.floor(rect.w + 0.5)
                local h = math.floor(rect.h + 0.5)
                if w > 0 and h > 0 then
                    rect = { x = math.floor(rect.x + 0.5), y = math.floor(rect.y + 0.5), w = w, h = h }
                    if self:drawSticker(bb, sticker, rect, faded, marker) then
                        self.hits[#self.hits + 1] = {
                            id = sticker.id,
                            kind = marker and "marker" or "sticker",
                            x = rect.x, y = rect.y, w = w, h = h,
                        }
                    end
                end
            end
        end
    end
end

return Layer
