--[[--
Book card for Kaia Home.

Two sizes:
- "hero": cover on the left, title, author and progress on the right.
- "shelf": cover above the title, for the recent row.

A book without a cover shows a framed placeholder with its title. Covers keep
their aspect ratio and fit inside the cover box. Tapping the card calls
`callback(book)`.

@module kaia.widget.bookcard
]]

local CenterContainer = require("ui/widget/container/centercontainer")
local FrameContainer = require("ui/widget/container/framecontainer")
local Geom = require("ui/geometry")
local GestureRange = require("ui/gesturerange")
local HorizontalGroup = require("ui/widget/horizontalgroup")
local HorizontalSpan = require("ui/widget/horizontalspan")
local ImageWidget = require("ui/widget/imagewidget")
local InputContainer = require("ui/widget/container/inputcontainer")
local ProgressBar = require("kaia/widget/progressbar")
local TextBoxWidget = require("ui/widget/textboxwidget")
local TextWidget = require("ui/widget/textwidget")
local VerticalGroup = require("ui/widget/verticalgroup")
local VerticalSpan = require("ui/widget/verticalspan")
local Theme = require("kaia/theme")

-- Lines shown for a title before it is cut with an ellipsis.
local TITLE_LINES = 2

local BookCard = InputContainer:extend{
    -- Book table from kaia/bookdata: file, title, authors, percent, cover_bb.
    book = nil,
    -- "hero" or "shelf".
    size = "shelf",
    -- Full width of the card.
    width = 0,
    -- Width of the cover. Defaults to the full width.
    cover_w = nil,
    callback = nil,
}

-- Returns the size that fits (src_w, src_h) inside (max_w, max_h) with the
-- aspect ratio kept.
local function fitBox(src_w, src_h, max_w, max_h)
    local scale = math.min(max_w / src_w, max_h / src_h)
    return math.max(1, math.floor(src_w * scale + 0.5)), math.max(1, math.floor(src_h * scale + 0.5))
end

-- Height of `lines` lines of text in this face. Matches TextBoxWidget's line height.
local function linesHeight(face, lines)
    return lines * math.floor((1 + 0.3) * face.size + 0.5)
end

-- Text box that wraps to at most `lines` lines. The last line ends with an ellipsis.
local function titleBox(text, face, width, lines, align)
    return TextBoxWidget:new{
        text = text,
        face = face,
        fgcolor = Theme.color.ink,
        width = width,
        alignment = align or "left",
        height = linesHeight(face, lines),
        height_adjust = true,
        height_overflow_show_ellipsis = true,
    }
end

-- A cover box of fixed size: the cover image if present, else a framed title.
local function coverBox(book, box_w, box_h)
    local border = Theme.line.hairline
    local inner_w = box_w - 2 * border
    local inner_h = box_h - 2 * border
    local inner
    if book.cover_bb then
        local fit_w, fit_h = fitBox(book.cover_bb:getWidth(), book.cover_bb:getHeight(), inner_w, inner_h)
        inner = FrameContainer:new{
            bordersize = border,
            color = Theme.color.divider,
            padding = 0,
            margin = 0,
            ImageWidget:new{
                image = book.cover_bb,
                image_disposable = false,
                width = fit_w,
                height = fit_h,
            },
        }
    else
        local pad = Theme.space.sm
        local text_w = inner_w - 2 * pad
        local text_h = inner_h - 2 * pad
        local face = Theme:font("book_title")
        local lines = math.max(1, math.floor(text_h / linesHeight(face, 1)))
        inner = FrameContainer:new{
            bordersize = border,
            color = Theme.color.divider,
            background = Theme.color.surface_alt,
            padding = pad,
            margin = 0,
            CenterContainer:new{
                dimen = Geom:new{ w = text_w, h = text_h },
                titleBox(book.title, face, text_w, lines, "center"),
            },
        }
    end
    return CenterContainer:new{
        dimen = Geom:new{ w = box_w, h = box_h },
        inner,
    }
end

function BookCard:init()
    local book = self.book
    local cover_w = self.cover_w or self.width
    local cover_h = math.floor(cover_w * 3 / 2)
    local column
    if self.size == "hero" then
        local gap = Theme.space.md
        local text_w = math.max(1, self.width - cover_w - gap)
        local info = VerticalGroup:new{
            align = "left",
            titleBox(book.title, Theme:font("book_title"), text_w, TITLE_LINES),
        }
        if book.authors then
            table.insert(info, VerticalSpan:new{ width = Theme.space.xs })
            table.insert(info, TextWidget:new{
                text = book.authors,
                face = Theme:font("caption"),
                fgcolor = Theme.color.ink_secondary,
                max_width = text_w,
            })
        end
        if book.percent then
            table.insert(info, VerticalSpan:new{ width = Theme.space.md })
            table.insert(info, ProgressBar:new{
                width = text_w,
                fraction = book.percent,
            })
        end
        column = HorizontalGroup:new{
            align = "top",
            coverBox(book, cover_w, cover_h),
            HorizontalSpan:new{ width = gap },
            info,
        }
    else
        column = VerticalGroup:new{
            align = "left",
            coverBox(book, cover_w, cover_h),
            VerticalSpan:new{ width = Theme.space.xs },
            titleBox(book.title, Theme:font("book_title"), cover_w, TITLE_LINES),
        }
    end
    self[1] = column
    self.ges_events = {
        TapBookCard = {
            GestureRange:new{
                ges = "tap",
                range = function() return self.dimen end,
            },
        },
    }
end

function BookCard:onFocus()
    self._focused = true
    return true
end

function BookCard:onUnfocus()
    self._focused = nil
    return true
end

function BookCard:paintTo(bb, x, y)
    InputContainer.paintTo(self, bb, x, y)
    if self._focused then
        local size = self:getSize()
        bb:paintBorder(x, y, size.w, size.h, Theme.line.focus, Theme.color.ink)
    end
end

function BookCard:onTapBookCard()
    if self.callback then
        self.callback(self.book)
    end
    return true
end

return BookCard
