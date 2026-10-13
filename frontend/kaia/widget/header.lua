--[[--
Surface header: optional back action on the left, title, battery level and an
optional action icon on the right.

The header is always `touch_target` tall, so every surface shares one header
height whether or not it has actions. The battery value is fixed when the
header is built. It only changes when the surface repaints, so Home never
redraws on a timer.

Options:
- `back_callback`: shows a chevron before the title and calls this on tap.
  The chevron points the other way for right-to-left layouts.
- `right_icon`, `right_callback`: icon after the battery text. The older
  `icon` and `icon_callback` names still work.

@module kaia.widget.header
]]

local BD = require("ui/bidi")
local CenterContainer = require("ui/widget/container/centercontainer")
local Geom = require("ui/geometry")
local GestureRange = require("ui/gesturerange")
local HorizontalGroup = require("ui/widget/horizontalgroup")
local HorizontalSpan = require("ui/widget/horizontalspan")
local IconWidget = require("ui/widget/iconwidget")
local InputContainer = require("ui/widget/container/inputcontainer")
local TextWidget = require("ui/widget/textwidget")
local WidgetContainer = require("ui/widget/container/widgetcontainer")
local T = require("ffi/util").template
local Theme = require("kaia/theme")
local _ = require("gettext")

-- Icon centered in a touch-sized tap area.
local TapIcon = InputContainer:extend{
    icon = nil,
    callback = nil,
}

function TapIcon:init()
    local box = Theme.touch_target
    local icon_size = math.floor(Theme.nav_height * 0.4)
    self.dimen = Geom:new{ x = 0, y = 0, w = box, h = box }
    self[1] = CenterContainer:new{
        dimen = Geom:new{ w = box, h = box },
        IconWidget:new{
            icon = self.icon,
            width = icon_size,
            height = icon_size,
            alpha = true,
        },
    }
    self.ges_events = {
        TapTapIcon = {
            GestureRange:new{
                ges = "tap",
                range = function() return self.dimen end,
            },
        },
    }
end

function TapIcon:onTapTapIcon()
    if self.callback then
        self.callback()
    end
    return true
end

local Header = WidgetContainer:extend{
    title = "",
    width = 0,
    -- Battery level from 0 to 100, or nil to hide it.
    battery_percent = nil,
    -- Back action on the left. Nil hides the chevron.
    back_callback = nil,
    -- Action icon on the right, and the function it calls. Nil hides it.
    right_icon = nil,
    right_callback = nil,
    -- Older names for right_icon and right_callback.
    icon = nil,
    icon_callback = nil,
}

function Header:init()
    local right = HorizontalGroup:new{ align = "center" }
    if self.battery_percent then
        table.insert(right, TextWidget:new{
            text = T(_("%1%"), self.battery_percent),
            face = Theme:font("caption"),
            fgcolor = Theme.color.ink_secondary,
        })
    end
    local right_icon = self.right_icon or self.icon
    local right_callback = self.right_callback or self.icon_callback
    if right_icon and right_callback then
        if #right > 0 then
            table.insert(right, HorizontalSpan:new{ width = Theme.space.sm })
        end
        table.insert(right, TapIcon:new{
            icon = right_icon,
            callback = right_callback,
        })
    end
    local right_w = right:getSize().w

    local left = HorizontalGroup:new{ align = "center" }
    if self.back_callback then
        -- The chevron points toward the start of the line, so it flips for RTL.
        table.insert(left, TapIcon:new{
            icon = BD.mirroredUILayout() and "chevron.right" or "chevron.left",
            callback = self.back_callback,
        })
    end
    local left_w = left:getSize().w

    local title = TextWidget:new{
        text = self.title,
        face = Theme:font("title"),
        fgcolor = Theme.color.ink,
        max_width = math.max(1, self.width - left_w - right_w - Theme.space.md),
    }
    local filler_w = math.max(0, self.width - left_w - title:getSize().w - right_w)

    local row = HorizontalGroup:new{ align = "center" }
    if BD.mirroredUILayout() then
        table.insert(row, right)
        table.insert(row, HorizontalSpan:new{ width = filler_w })
        table.insert(row, title)
        table.insert(row, left)
    else
        table.insert(row, left)
        table.insert(row, title)
        table.insert(row, HorizontalSpan:new{ width = filler_w })
        table.insert(row, right)
    end
    self[1] = CenterContainer:new{
        dimen = Geom:new{ w = self.width, h = Theme.touch_target },
        row,
    }
end

--- Height of the header. Constant for every surface.
function Header:getSize()
    return Geom:new{ w = self.width, h = Theme.touch_target }
end

return Header
