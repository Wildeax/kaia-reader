--[[--
Two-line list row for Notes: primary text, secondary text and a hairline divider.

Text is truncated to one line each. Padding is `space.md` on both sides.

@module kaia.widget.listrow
]]

local Device = require("device")
local Geom = require("ui/geometry")
local GestureRange = require("ui/gesturerange")
local HorizontalGroup = require("ui/widget/horizontalgroup")
local HorizontalSpan = require("ui/widget/horizontalspan")
local InputContainer = require("ui/widget/container/inputcontainer")
local LineWidget = require("ui/widget/linewidget")
local TextWidget = require("ui/widget/textwidget")
local VerticalGroup = require("ui/widget/verticalgroup")
local VerticalSpan = require("ui/widget/verticalspan")
local Theme = require("kaia/theme")

local Screen = Device.screen

local ListRow = InputContainer:extend{
    width = nil,
    primary = "",
    secondary = nil,
    callback = nil,
}

function ListRow:init()
    self.width = self.width or Screen:getWidth()
    local text_w = math.max(1, self.width - 2 * Theme.space.md)
    local column = VerticalGroup:new{
        align = "left",
        VerticalSpan:new{ width = Theme.space.md },
        TextWidget:new{
            text = self.primary,
            face = Theme:font("body"),
            fgcolor = Theme.color.ink,
            max_width = text_w,
        },
    }
    if self.secondary and self.secondary ~= "" then
        table.insert(column, VerticalSpan:new{ width = Theme.space.xs })
        table.insert(column, TextWidget:new{
            text = self.secondary,
            face = Theme:font("caption"),
            fgcolor = Theme.color.ink_secondary,
            max_width = text_w,
        })
    end
    table.insert(column, VerticalSpan:new{ width = Theme.space.md })
    table.insert(column, LineWidget:new{
        dimen = Geom:new{ w = self.width, h = Theme.line.divider },
        background = Theme.color.divider,
    })

    self[1] = HorizontalGroup:new{
        align = "top",
        HorizontalSpan:new{ width = Theme.space.md },
        column,
    }
    self.dimen = Geom:new{ x = 0, y = 0, w = self.width, h = self:getSize().h }
    if self.callback then
        self.ges_events = {
            TapListRow = {
                GestureRange:new{
                    ges = "tap",
                    range = function() return self.dimen end,
                },
            },
        }
    end
end

function ListRow:onTapListRow()
    if self.callback then
        self.callback()
    end
    return true
end

return ListRow
