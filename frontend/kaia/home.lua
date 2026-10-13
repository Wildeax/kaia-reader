--[[--
Kaia Home: the start surface.

Home is a full screen widget shown on top of the file manager. It shows the
battery level, the book to continue, the recent books and a count line. The
nav bar sits at the bottom. Home starts no timers and redraws only when it is
shown or refreshed.

Usage:
    require("kaia/home"):show()

@module kaia.home
]]

local BookCard = require("kaia/widget/bookcard")
local BookData = require("kaia/bookdata")
local Button = require("ui/widget/button")
local Device = require("device")
local FocusManager = require("ui/widget/focusmanager")
local FrameContainer = require("ui/widget/container/framecontainer")
local Geom = require("ui/geometry")
local GestureRange = require("ui/gesturerange")
local Header = require("kaia/widget/header")
local HorizontalGroup = require("ui/widget/horizontalgroup")
local HorizontalSpan = require("ui/widget/horizontalspan")
local InfoMessage = require("ui/widget/infomessage")
local InputContainer = require("ui/widget/container/inputcontainer")
local NavBar = require("kaia/widget/navbar")
local TextWidget = require("ui/widget/textwidget")
local Theme = require("kaia/theme")
local UIManager = require("ui/uimanager")
local VerticalGroup = require("ui/widget/verticalgroup")
local VerticalSpan = require("ui/widget/verticalspan")
local logger = require("logger")
local _ = require("gettext")
local T = require("ffi/util").template

local Screen = Device.screen

-- Books in the recent row, below the hero card.
local SHELF_COUNT = 4
-- Hero cover width as a fraction of the content width.
local HERO_COVER_FRACTION = 0.24

local Home = FocusManager:extend{
    name = "kaia_home",
    covers_fullscreen = true,
    -- Optional data injection for tests. Nil uses ReadHistory and DocSettings.
    history = nil,
    metadata = nil,
}

-- The Home instance on the widget stack. Only one Home is shown at a time.
local current

-- Area that calls `callback` on tap. Used for the count line.
local TapArea = InputContainer:extend{
    callback = nil,
}

function TapArea:init()
    self.ges_events = {
        TapArea = {
            GestureRange:new{
                ges = "tap",
                range = function() return self.dimen end,
            },
        },
    }
end

function TapArea:onTapArea()
    if self.callback then
        self.callback()
    end
    return true
end

local function sectionLabel(text)
    return TextWidget:new{
        text = text,
        face = Theme:font("label"),
        fgcolor = Theme.color.ink_secondary,
    }
end

--- Shows Home on top of the file manager. Replaces an open Home.
function Home:show()
    if current then
        UIManager:close(current)
        current = nil
    end
    local widget = self:new{}
    current = widget
    UIManager:show(widget)
    return widget
end

function Home:init()
    self.dimen = Geom:new{ x = 0, y = 0, w = Screen:getWidth(), h = Screen:getHeight() }
    if Device:hasKeys() then
        self.key_events.Close = { { Device.input.group.Back } }
    end
    self:buildContent()
end

--- Rebuilds the page from current data and repaints Home.
function Home:refresh()
    self:buildContent()
    UIManager:setDirty(self, "ui")
end

function Home:buildContent()
    local screen_w, screen_h = self.dimen.w, self.dimen.h
    local margin = Theme.space.lg
    local content_w = screen_w - 2 * margin
    local gap = Theme.space.md

    local data = { history = self.history, metadata = self.metadata }
    local books = BookData.recent(1 + SHELF_COUNT, data)
    local counts = BookData.counts(data)
    local power = Device:hasBattery() and Device:getPowerDevice():getCapacity() or nil

    -- Rows the focus manager moves through, top to bottom.
    self.selected = { x = 1, y = 1 }
    self.layout = {}

    local body = VerticalGroup:new{ align = "left" }
    table.insert(body, Header:new{
        title = "Kaia",
        width = content_w,
        battery_percent = power,
    })
    table.insert(body, VerticalSpan:new{ width = Theme.space.md })

    if #books == 0 then
        local button = Button:new{
            text = _("Open library"),
            width = math.floor(content_w / 2),
            height = Theme.touch_target,
            text_font_face = Theme.fonts.body_bold[1],
            text_font_size = Theme.fonts.body_bold[2],
            show_parent = self,
            callback = function() self:openLibrary() end,
        }
        table.insert(body, TextWidget:new{
            text = _("No books yet"),
            face = Theme:font("title"),
            fgcolor = Theme.color.ink,
        })
        table.insert(body, VerticalSpan:new{ width = Theme.space.sm })
        table.insert(body, TextWidget:new{
            text = _("Open a book from the library and it will appear here."),
            face = Theme:font("caption"),
            fgcolor = Theme.color.ink_secondary,
            max_width = content_w,
        })
        table.insert(body, VerticalSpan:new{ width = Theme.space.md })
        table.insert(body, button)
        table.insert(self.layout, { button })
    else
        local hero = BookCard:new{
            book = books[1],
            size = "hero",
            width = content_w,
            cover_w = math.floor(content_w * HERO_COVER_FRACTION),
            callback = function(book) self:openBook(book.file) end,
        }
        table.insert(body, sectionLabel(_("Continue reading")))
        table.insert(body, VerticalSpan:new{ width = Theme.space.sm })
        table.insert(body, hero)
        table.insert(self.layout, { hero })

        if #books > 1 then
            local card_w = math.floor((content_w - (SHELF_COUNT - 1) * gap) / SHELF_COUNT)
            local shelf = HorizontalGroup:new{ align = "top" }
            local shelf_row = {}
            for i = 2, #books do
                if i > 2 then
                    table.insert(shelf, HorizontalSpan:new{ width = gap })
                end
                local card = BookCard:new{
                    book = books[i],
                    size = "shelf",
                    width = card_w,
                    callback = function(book) self:openBook(book.file) end,
                }
                table.insert(shelf, card)
                table.insert(shelf_row, card)
            end
            table.insert(body, VerticalSpan:new{ width = Theme.space.md })
            table.insert(body, sectionLabel(_("Recent")))
            table.insert(body, VerticalSpan:new{ width = Theme.space.sm })
            table.insert(body, shelf)
            table.insert(self.layout, shelf_row)
        end
    end

    table.insert(body, VerticalSpan:new{ width = Theme.space.md })
    local count_text = TextWidget:new{
        text = T(_("Books in history: %1"), counts.books_in_history),
        face = Theme:font("body"),
        fgcolor = Theme.color.ink,
    }
    table.insert(body, TapArea:new{
        dimen = Geom:new{ w = content_w, h = count_text:getSize().h },
        callback = function() self:openLibrary() end,
        count_text,
    })

    local navbar = NavBar:new{
        width = screen_w,
        active = "home",
        callbacks = {
            home = function() self:refresh() end,
            library = function() self:openLibrary() end,
            notes = function() self:openNotes() end,
            settings = function() self:openSettings() end,
        },
    }
    for _i, row in ipairs(navbar.layout) do
        table.insert(self.layout, row)
    end

    local upper = FrameContainer:new{
        bordersize = 0,
        padding = 0,
        padding_top = Theme.space.md,
        padding_left = margin,
        padding_right = margin,
        margin = 0,
        body,
    }
    local gap_h = math.max(0, screen_h - upper:getSize().h - navbar:getSize().h)
    local page = VerticalGroup:new{
        align = "left",
        upper,
        VerticalSpan:new{ width = gap_h },
        navbar,
    }
    self[1] = FrameContainer:new{
        width = screen_w,
        height = screen_h,
        bordersize = 0,
        padding = 0,
        margin = 0,
        background = Theme.color.surface,
        page,
    }
end

function Home:onClose()
    UIManager:close(self)
    return true
end

function Home:onCloseWidget()
    if current == self then
        current = nil
    end
end

--- Closes Home and opens the book in the reader.
function Home:openBook(file)
    self:onClose()
    require("apps/reader/readerui"):showReader(file)
end

--- Closes Home, which shows the file manager underneath.
function Home:openLibrary()
    self:onClose()
    local FileManager = require("apps/filemanager/filemanager")
    if not FileManager.instance then
        FileManager:showFiles()
    end
end

--- Opens the file manager main menu. Home stays underneath it.
function Home:openSettings()
    local FileManager = require("apps/filemanager/filemanager")
    if FileManager.instance and FileManager.instance.menu then
        FileManager.instance.menu:onShowMenu()
    end
end

--- Opens Notes once kaia/notes exists. Until then it shows a short message.
function Home:openNotes()
    local ok, Notes = pcall(require, "kaia/notes")
    if ok then
        Notes:show()
        return
    end
    if not tostring(Notes):find("not found", 1, true) then
        logger.warn("kaia/notes failed to load:", Notes)
    end
    UIManager:show(InfoMessage:new{ text = _("Notes are not available yet.") })
end

return Home
