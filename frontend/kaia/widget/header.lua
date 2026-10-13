--[[--
Surface header: title on the left, battery level and a settings icon on the right.

The battery value is fixed when the header is built. It only changes when the
surface repaints, so Home never redraws on a timer.

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
    -- Icon on the far right, and the function it calls. Nil hides the icon.
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
    if self.icon and self.icon_callback then
        if #right > 0 then
            table.insert(right, HorizontalSpan:new{ width = Theme.space.sm })
        end
        table.insert(right, TapIcon:new{
            icon = self.icon,
            callback = self.icon_callback,
        })
    end
    local right_w = right:getSize().w

    local title = TextWidget:new{
        text = self.title,
        face = Theme:font("title"),
        fgcolor = Theme.color.ink,
        max_width = math.max(1, self.width - right_w - Theme.space.md),
    }
    local filler_w = math.max(0, self.width - title:getSize().w - right_w)

    local row = HorizontalGroup:new{ align = "center" }
    if BD.mirroredUILayout() then
        table.insert(row, right)
        table.insert(row, HorizontalSpan:new{ width = filler_w })
        table.insert(row, title)
    else
        table.insert(row, title)
        table.insert(row, HorizontalSpan:new{ width = filler_w })
        table.insert(row, right)
    end
    self[1] = row
end

return Header
