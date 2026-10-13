--[[--
Surface header: title on the left, battery level on the right.

The battery value is fixed when the header is built. It only changes when the
surface repaints, so Home never redraws on a timer.

@module kaia.widget.header
]]

local BD = require("ui/bidi")
local HorizontalGroup = require("ui/widget/horizontalgroup")
local HorizontalSpan = require("ui/widget/horizontalspan")
local TextWidget = require("ui/widget/textwidget")
local WidgetContainer = require("ui/widget/container/widgetcontainer")
local T = require("ffi/util").template
local Theme = require("kaia/theme")
local _ = require("gettext")

local Header = WidgetContainer:extend{
    title = "",
    width = 0,
    -- Battery level from 0 to 100, or nil to hide it.
    battery_percent = nil,
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
