--[[--
Bottom navigation bar: Home, Library, Notes, Settings.

Each item has an icon and a label. The active item has a bold label and a
`line.focus` underline in the accent color. Items share the width equally.
The bar exposes `layout` for FocusManager, so page-turn keys move between items.

@module kaia.widget.navbar
]]

local CenterContainer = require("ui/widget/container/centercontainer")
local Device = require("device")
local Geom = require("ui/geometry")
local GestureRange = require("ui/gesturerange")
local HorizontalGroup = require("ui/widget/horizontalgroup")
local IconWidget = require("ui/widget/iconwidget")
local InputContainer = require("ui/widget/container/inputcontainer")
local TextWidget = require("ui/widget/textwidget")
local VerticalGroup = require("ui/widget/verticalgroup")
local VerticalSpan = require("ui/widget/verticalspan")
local Widget = require("ui/widget/widget")
local WidgetContainer = require("ui/widget/container/widgetcontainer")
local Theme = require("kaia/theme")
local _ = require("gettext")

local Screen = Device.screen

-- Filled rectangle. A nil color means the accent (ink on grayscale panels).
local Rule = Widget:extend{
    width = 0,
    height = 0,
    color = nil,
}

function Rule:getSize()
    return Geom:new{ w = self.width, h = self.height }
end

function Rule:paintTo(bb, x, y)
    if self.color then
        bb:paintRect(x, y, self.width, self.height, self.color)
    elseif Screen:isColorEnabled() then
        bb:paintRectRGB32(x, y, self.width, self.height, Theme:accent())
    else
        bb:paintRect(x, y, self.width, self.height, Theme.color.ink)
    end
end

local NavItem = InputContainer:extend{
    width = 0,
    height = 0,
    icon = nil,
    label = nil,
    active = false,
    callback = nil,
}

function NavItem:init()
    local icon_size = math.floor(Theme.touch_target * 0.6)
    local label = TextWidget:new{
        text = self.label,
        face = Theme:font(self.active and "label" or "caption"),
        fgcolor = self.active and Theme.color.ink or Theme.color.ink_secondary,
    }
    local column = VerticalGroup:new{
        align = "center",
        IconWidget:new{
            icon = self.icon,
            width = icon_size,
            height = icon_size,
        },
        VerticalSpan:new{ width = Theme.space.xs },
        label,
        VerticalSpan:new{ width = Theme.space.xs },
    }
    if self.active then
        table.insert(column, Rule:new{
            width = self.width - 2 * Theme.space.md,
            height = Theme.line.focus,
        })
    else
        table.insert(column, VerticalSpan:new{ width = Theme.line.focus })
    end
    self.dimen = Geom:new{ x = 0, y = 0, w = self.width, h = self.height }
    self[1] = CenterContainer:new{
        dimen = self.dimen:copy(),
        column,
    }
    self.ges_events = {
        TapNavItem = {
            GestureRange:new{
                ges = "tap",
                range = function() return self.dimen end,
            },
        },
    }
end

function NavItem:onFocus()
    self._focused = true
    return true
end

function NavItem:onUnfocus()
    self._focused = nil
    return true
end

function NavItem:paintTo(bb, x, y)
    InputContainer.paintTo(self, bb, x, y)
    if self._focused then
        bb:paintBorder(x, y, self.width, self.height, Theme.line.focus, Theme.color.ink)
    end
end

function NavItem:onTapNavItem()
    if self.callback then
        self.callback()
    end
    return true
end

local NavBar = WidgetContainer:extend{
    width = 0,
    -- Key of the surface being shown: "home", "library", "notes" or "settings".
    active = "home",
    -- Table of callbacks keyed by item name.
    callbacks = nil,
}

local function itemDefs()
    return {
        { key = "home", label = _("Home"), icon = "home" },
        { key = "library", label = _("Library"), icon = "appbar.filebrowser" },
        { key = "notes", label = _("Notes"), icon = "bookmark" },
        { key = "settings", label = _("Settings"), icon = "appbar.settings" },
    }
end

function NavBar:init()
    local defs = itemDefs()
    local count = #defs
    local base_w = math.floor(self.width / count)
    local row = HorizontalGroup:new{ align = "center" }
    self.layout = { {} }
    for i, def in ipairs(defs) do
        local w = (i == count) and (self.width - base_w * (count - 1)) or base_w
        local item = NavItem:new{
            width = w,
            height = Theme.nav_height,
            icon = def.icon,
            label = def.label,
            active = def.key == self.active,
            callback = self.callbacks and self.callbacks[def.key],
        }
        table.insert(row, item)
        self.layout[1][i] = item
    end
    self[1] = VerticalGroup:new{
        Rule:new{ width = self.width, height = Theme.line.divider, color = Theme.color.divider },
        row,
    }
end

return NavBar
