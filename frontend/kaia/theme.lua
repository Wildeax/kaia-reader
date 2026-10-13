--[[--
Kaia design tokens. See docs/kaia/specs/DESIGN.md.

Widgets read sizes, fonts and colors from here instead of hardcoding them.
Sizes are scaled for the current screen when this module loads.

@module kaia.theme
]]

local Blitbuffer = require("ffi/blitbuffer")
local Device = require("device")
local Font = require("ui/font")
local Screen = Device.screen

local function s(v) return Screen:scaleBySize(v) end

local Theme = {
    space = {
        xs = s(4),
        sm = s(8),
        md = s(16),
        lg = s(24),
        xl = s(40),
    },
    radius = {
        card = s(12),
        button = s(10),
    },
    line = {
        hairline = s(1),
        divider = s(1),
        focus = s(3),
        progress = s(6),
    },
    -- Font face file and size; use Theme:font(name) to get a face.
    fonts = {
        display = { "NotoSerif-Bold.ttf", 34 },
        title = { "NotoSans-Bold.ttf", 26 },
        book_title = { "NotoSerif-Bold.ttf", 22 },
        body = { "NotoSans-Regular.ttf", 20 },
        body_bold = { "NotoSans-Bold.ttf", 20 },
        shelf_title = { "NotoSans-Bold.ttf", 16 },
        caption = { "NotoSans-Regular.ttf", 16 },
        label = { "NotoSans-Bold.ttf", 15 },
    },
    color = {
        ink = Blitbuffer.COLOR_BLACK,
        ink_secondary = Blitbuffer.Color8(0x55),
        ink_muted = Blitbuffer.Color8(0x88),
        divider = Blitbuffer.Color8(0xCC),
        surface = Blitbuffer.COLOR_WHITE,
        surface_alt = Blitbuffer.Color8(0xEE),
    },
    -- scaleBySize maps 1 unit to about 2.1 px on a 300 PPI panel.
    -- Minimum touch target: 50 units, about 106 px or 9 mm on the Libra Colour.
    touch_target = s(50),
    nav_height = s(60),
    -- Cover heights. Widths follow each cover's aspect ratio.
    cover = {
        hero_h = s(210),
        shelf_h = s(140),
    },
}

--- Returns a font face for a token name such as "title" or "caption".
function Theme:font(name)
    local f = self.fonts[name]
    assert(f, "unknown Kaia font token: " .. tostring(name))
    return Font:getFace(f[1], f[2])
end

--- Accent color: green on color panels, ink otherwise.
-- Evaluated on each call because color rendering can be toggled at runtime.
function Theme:accent()
    if Screen:isColorEnabled() then
        return Blitbuffer.ColorRGB32(0x1F, 0x6F, 0x5C, 0xFF)
    end
    return self.color.ink
end

return Theme
