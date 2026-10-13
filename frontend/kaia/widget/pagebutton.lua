--[[--
Pager button: a label in a hairline frame with rounded corners.

Enabled buttons use `ink` text. A disabled button uses `ink_muted` text and
does not call its callback. Focus draws a `line.focus` border for key navigation.

@module kaia.widget.pagebutton
]]

local CenterContainer = require("ui/widget/container/centercontainer")
local FrameContainer = require("ui/widget/container/framecontainer")
local Geom = require("ui/geometry")
local GestureRange = require("ui/gesturerange")
local InputContainer = require("ui/widget/container/inputcontainer")
local TextWidget = require("ui/widget/textwidget")
local Theme = require("kaia/theme")

local PageButton = InputContainer:extend{
    text = "",
    width = 0,
    height = Theme.touch_target,
    enabled = true,
    callback = nil,
}

function PageButton:init()
    local bordersize = Theme.line.hairline
    local label = TextWidget:new{
        text = self.text,
        face = Theme:font("label"),
        fgcolor = self.enabled and Theme.color.ink or Theme.color.ink_muted,
        max_width = math.max(1, self.width - 2 * Theme.space.md),
    }
    self.dimen = Geom:new{ x = 0, y = 0, w = self.width, h = self.height }
    self[1] = FrameContainer:new{
        width = self.width,
        height = self.height,
        padding = 0,
        margin = 0,
        bordersize = bordersize,
        color = Theme.color.divider,
        radius = Theme.radius.button,
        CenterContainer:new{
            dimen = Geom:new{ w = self.width - 2 * bordersize, h = self.height - 2 * bordersize },
            label,
        },
    }
    self.ges_events = {
        TapPageButton = {
            GestureRange:new{
                ges = "tap",
                range = function() return self.dimen end,
            },
        },
    }
end

function PageButton:onFocus()
    self._focused = true
    return true
end

function PageButton:onUnfocus()
    self._focused = nil
    return true
end

function PageButton:paintTo(bb, x, y)
    InputContainer.paintTo(self, bb, x, y)
    if self._focused then
        bb:paintBorder(x, y, self.width, self.height, Theme.line.focus, Theme.color.ink)
    end
end

function PageButton:onTapPageButton()
    if self.enabled and self.callback then
        self.callback()
    end
    return true
end

return PageButton
