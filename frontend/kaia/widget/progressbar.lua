--[[--
Reading progress bar with a percent label.

The fill uses the accent color on color panels and ink otherwise. The percent
text sits next to the bar, so the value never depends on color alone.

@module kaia.widget.progressbar
]]

local BD = require("ui/bidi")
local Device = require("device")
local Geom = require("ui/geometry")
local HorizontalGroup = require("ui/widget/horizontalgroup")
local HorizontalSpan = require("ui/widget/horizontalspan")
local TextWidget = require("ui/widget/textwidget")
local Widget = require("ui/widget/widget")
local WidgetContainer = require("ui/widget/container/widgetcontainer")
local T = require("ffi/util").template
local Theme = require("kaia/theme")
local _ = require("gettext")

local Screen = Device.screen

-- The track and fill, drawn without a border.
local Bar = Widget:extend{
    width = 0,
    height = 0,
    fraction = 0,
}

function Bar:getSize()
    return Geom:new{ w = self.width, h = self.height }
end

function Bar:paintTo(bb, x, y)
    bb:paintRect(x, y, self.width, self.height, Theme.color.divider)
    local fill = math.floor(self.width * self.fraction + 0.5)
    if fill <= 0 then
        return
    end
    local fill_x = BD.mirroredUILayout() and (x + self.width - fill) or x
    if Screen:isColorEnabled() then
        bb:paintRectRGB32(fill_x, y, fill, self.height, Theme:accent())
    else
        bb:paintRect(fill_x, y, fill, self.height, Theme.color.ink)
    end
end

local ProgressBar = WidgetContainer:extend{
    -- Total width including the percent label.
    width = 0,
    -- Value from 0 to 1. Values outside the range are clamped.
    fraction = 0,
    -- Show the percent label when true.
    show_percent = true,
}

function ProgressBar:init()
    local fraction = math.max(0, math.min(1, self.fraction or 0))
    local children = HorizontalGroup:new{ align = "center" }
    local label
    if self.show_percent then
        label = TextWidget:new{
            text = T(_("%1%"), math.floor(fraction * 100 + 0.5)),
            face = Theme:font("caption"),
            fgcolor = Theme.color.ink,
        }
    end
    local gap = label and Theme.space.sm or 0
    local label_w = label and (label:getSize().w + gap) or 0
    local bar = Bar:new{
        width = math.max(1, self.width - label_w),
        height = Theme.line.progress,
        fraction = fraction,
    }
    if label and BD.mirroredUILayout() then
        table.insert(children, label)
        table.insert(children, HorizontalSpan:new{ width = gap })
        table.insert(children, bar)
    else
        table.insert(children, bar)
        if label then
            table.insert(children, HorizontalSpan:new{ width = gap })
            table.insert(children, label)
        end
    end
    self[1] = children
end

return ProgressBar
