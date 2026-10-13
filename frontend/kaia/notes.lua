--[[--
Kaia Notes: highlights, notes and bookmarks, grouped by book.

One full screen widget with two views. The book list shows every book that has
annotations, newest first. Tapping a book shows its entries in reading order.
Tapping an entry closes Notes and opens the book at that place. Either list
pages when it does not fit on one screen. Nothing is written to DocSettings.

Usage:
    require("kaia/notes"):show()

Data comes from kaia/notesdata. Sticker entries, when a provider is set, appear
under "Needs repair" (orphaned) and in each book's entries.

@module kaia.notes
]]

local BookData = require("kaia/bookdata")
local CenterContainer = require("ui/widget/container/centercontainer")
local Device = require("device")
local FocusManager = require("ui/widget/focusmanager")
local FrameContainer = require("ui/widget/container/framecontainer")
local Geom = require("ui/geometry")
local GestureRange = require("ui/gesturerange")
local Header = require("kaia/widget/header")
local Home = require("kaia/home")
local HorizontalGroup = require("ui/widget/horizontalgroup")
local HorizontalSpan = require("ui/widget/horizontalspan")
local InputContainer = require("ui/widget/container/inputcontainer")
local LineWidget = require("ui/widget/linewidget")
local ListRow = require("kaia/widget/listrow")
local NavBar = require("kaia/widget/navbar")
local NotesData = require("kaia/notesdata")
local PageButton = require("kaia/widget/pagebutton")
local TextBoxWidget = require("ui/widget/textboxwidget")
local TextWidget = require("ui/widget/textwidget")
local Theme = require("kaia/theme")
local UIManager = require("ui/uimanager")
local VerticalGroup = require("ui/widget/verticalgroup")
local VerticalSpan = require("ui/widget/verticalspan")
local _ = require("gettext")
local N_ = _.ngettext
local T = require("ffi/util").template

local Screen = Device.screen

-- Lines of quoted text per entry row before the text is cut with an ellipsis.
local QUOTE_LINES = 2

local Notes = FocusManager:extend{
    name = "kaia_notes",
    covers_fullscreen = true,
    -- Optional data injection for tests. Nil uses ReadHistory.
    history = nil,
}

-- The Notes instance on the widget stack. Only one Notes is shown at a time.
local current

-- Collapses whitespace and returns nil for empty text.
local function flatten(text)
    if type(text) ~= "string" then
        return nil
    end
    local s = text:gsub("%s+", " "):match("^%s*(.-)%s*$")
    if s == "" then
        return nil
    end
    return s
end

-- "Page 12" for a numeric page. EPUB locations are XPointer strings and have no page number.
local function pageLabel(entry)
    local page = entry.location and entry.location.page
    if type(page) == "number" then
        return T(_("Page %1"), page)
    end
end

-- Primary and secondary lines for an annotation.
-- Highlights and notes: the quote is the primary line, the note or chapter the secondary.
-- Bookmarks: "Bookmark", then the chapter or page.
-- Stickers: the caption or "Sticker", then the chapter or page.
local function entryLines(entry)
    if entry.kind == "bookmark" then
        return _("Bookmark"), flatten(entry.chapter) or pageLabel(entry)
    elseif entry.kind == "sticker" then
        local caption = flatten(entry.text) or flatten(entry.note) or _("Sticker")
        return caption, flatten(entry.chapter) or pageLabel(entry)
    end
    local quote = flatten(entry.text)
    local note = flatten(entry.note)
    if quote then
        return "“" .. quote .. "”", note or flatten(entry.chapter)
    end
    return flatten(entry.chapter) or _("Highlight"), note
end

-- "3 highlights, 1 note, 2 bookmarks". Zero counts are left out.
local function countsLine(book)
    local parts = {}
    local function add(n, one, many)
        if n > 0 then
            parts[#parts + 1] = T(N_(one, many, n), n)
        end
    end
    add(book.count_highlights, "1 highlight", "%1 highlights")
    add(book.count_notes, "1 note", "%1 notes")
    add(book.count_bookmarks, "1 bookmark", "%1 bookmarks")
    return table.concat(parts, ", ")
end

-- A row with a primary line and one secondary line. A quote takes up to
-- QUOTE_LINES lines. Other primary text (a label or a sticker caption) takes one.
-- The height depends only on `quote`, so paging can measure one row per kind.
-- Text starts at the content edge, and the divider spans the same width.
local EntryRow = InputContainer:extend{
    width = 0,
    primary = "",
    secondary = nil,
    quote = true,
    callback = nil,
}

-- Lines of primary text for a row.
local function primaryLines(quote)
    return quote and QUOTE_LINES or 1
end

function EntryRow:init()
    local text_w = math.max(1, self.width)
    local quote_face = Theme:font("body")
    -- Same line height as TextBoxWidget uses for this face.
    local line_h = math.floor((1 + 0.3) * quote_face.size + 0.5)
    local column = VerticalGroup:new{
        align = "left",
        VerticalSpan:new{ width = Theme.space.md },
        TextBoxWidget:new{
            text = self.primary,
            face = quote_face,
            fgcolor = Theme.color.ink,
            width = text_w,
            height = line_h * primaryLines(self.quote),
            height_adjust = false,
            height_overflow_show_ellipsis = true,
        },
        VerticalSpan:new{ width = Theme.space.xs },
        TextWidget:new{
            text = self.secondary or " ",
            face = Theme:font("caption"),
            fgcolor = Theme.color.ink_secondary,
            max_width = text_w,
        },
        VerticalSpan:new{ width = Theme.space.md },
        LineWidget:new{
            dimen = Geom:new{ w = self.width, h = Theme.line.divider },
            background = Theme.color.divider,
        },
    }
    self[1] = column
    self.dimen = Geom:new{ x = 0, y = 0, w = self.width, h = self:getSize().h }
    self.ges_events = {
        TapEntryRow = {
            GestureRange:new{
                ges = "tap",
                range = function() return self.dimen end,
            },
        },
    }
end

function EntryRow:onFocus()
    self._focused = true
    return true
end

function EntryRow:onUnfocus()
    self._focused = nil
    return true
end

function EntryRow:paintTo(bb, x, y)
    InputContainer.paintTo(self, bb, x, y)
    if self._focused then
        local size = self:getSize()
        bb:paintBorder(x, y, size.w, size.h, Theme.line.focus, Theme.color.ink)
    end
end

function EntryRow:onTapEntryRow()
    if self.callback then
        self.callback()
    end
    return true
end

local function sectionLabel(text, width)
    return VerticalGroup:new{
        align = "left",
        VerticalSpan:new{ width = Theme.space.md },
        TextWidget:new{
            text = text,
            face = Theme:font("label"),
            fgcolor = Theme.color.ink_secondary,
            max_width = width,
        },
        VerticalSpan:new{ width = Theme.space.xs },
    }
end

-- Height of the widget that `build` returns.
local function measure(build)
    return build():getSize().h
end

-- Splits items into pages. Each item has a `height` and is kept whole.
local function paginate(items, avail)
    local pages = {}
    local page, used = {}, 0
    for _k, item in ipairs(items) do
        if #page > 0 and used + item.height > avail then
            pages[#pages + 1] = page
            page, used = {}, 0
        end
        page[#page + 1] = item
        used = used + item.height
    end
    if #page > 0 then
        pages[#pages + 1] = page
    end
    return pages
end

-- Closes this Notes and the Home widget, if Home is shown. Home sits under Notes
-- when Notes is opened from Home, and it must not reappear after a jump to a book.
local function closeKaiaSurfaces(notes)
    UIManager:close(notes)
    Home.closeIfShown()
end

--- Shows Notes on top of the current screen. Replaces an open Notes.
-- @table opts optional; `history` is passed to kaia/notesdata for tests.
function Notes:show(opts)
    if current then
        UIManager:close(current)
        current = nil
    end
    local widget = self:new{ history = opts and opts.history }
    current = widget
    UIManager:show(widget)
    return widget
end

function Notes:init()
    self.dimen = Geom:new{ x = 0, y = 0, w = Screen:getWidth(), h = Screen:getHeight() }
    if Device:hasKeys() then
        self.key_events.Close = { { Device.input.group.Back } }
    end
    self.books = NotesData.books(self.history)
    self.book = nil
    self.entries = nil
    self.page = 1
    self:buildContent()
end

--- Rebuilds the page from current data and repaints Notes.
function Notes:refresh()
    self:buildContent()
    UIManager:setDirty(self, "ui")
end

--- Shows the entries of one book. Entries are read from the sidecar once.
function Notes:showBook(book)
    self.book = book
    self.entries = self:loadEntries(book.file)
    self.page = 1
    self:refresh()
end

--- Returns to the book list.
function Notes:showBooks()
    self.book = nil
    self.entries = nil
    self.page = 1
    self:refresh()
end

function Notes:turnPage(delta)
    self.page = self.page + delta
    self:refresh()
end

--- Annotations of one book in reading order, with sticker entries added last.
function Notes:loadEntries(file)
    local list = NotesData.entries(file)
    local stickers = NotesData.stickers()
    local own = stickers and stickers.by_file and stickers.by_file[file] or {}
    for _k, sticker in ipairs(own) do
        list[#list + 1] = sticker
    end
    return list
end

-- Rows of the book list or the entry list, each with its height and builder.
-- Heights are measured from sample rows, so paging matches what gets drawn.
function Notes:listItems(content_w)
    local items = {}
    -- Non-focusable items are headings. They take space but get no focus.
    local function add(height, build, focusable)
        items[#items + 1] = { height = height, build = build, focusable = focusable ~= false }
    end

    if self.book then
        local heights = {}
        for _k, quote in ipairs({ true, false }) do
            heights[quote] = measure(function()
                return EntryRow:new{ width = content_w, primary = "Ag", secondary = "Ag", quote = quote }
            end)
        end
        for _k, entry in ipairs(self.entries or {}) do
            local primary, secondary = entryLines(entry)
            local quote = entry.kind ~= "bookmark" and entry.kind ~= "sticker" and flatten(entry.text) ~= nil
            add(heights[quote], function()
                return EntryRow:new{
                    width = content_w,
                    primary = primary,
                    secondary = secondary,
                    quote = quote,
                    callback = function() self:openEntry(self.book.file, entry) end,
                }
            end)
        end
        return items
    end

    local stickers = NotesData.stickers()
    local orphaned = stickers and stickers.orphaned or {}
    if #orphaned > 0 then
        local label_h = measure(function() return sectionLabel(_("Needs repair"), content_w) end)
        add(label_h, function() return sectionLabel(_("Needs repair"), content_w) end, false)
        local repair_h = measure(function()
            return EntryRow:new{ width = content_w, primary = "Ag", secondary = "Ag" }
        end)
        for _k, entry in ipairs(orphaned) do
            local primary = entryLines(entry)
            local source = entry.file and BookData.baseTitle(entry.file) or nil
            add(repair_h, function()
                return EntryRow:new{
                    width = content_w,
                    primary = primary,
                    secondary = source,
                    callback = function() self:openEntry(entry.file, entry) end,
                }
            end)
        end
    end

    local row_h = measure(function()
        return ListRow:new{ width = content_w, primary = "Ag", secondary = "Ag" }
    end)
    for _k, book in ipairs(self.books) do
        add(row_h, function()
            return ListRow:new{
                width = content_w,
                primary = book.title,
                secondary = countsLine(book),
                callback = function() self:showBook(book) end,
            }
        end)
    end
    return items
end

function Notes:buildContent()
    local screen_w, screen_h = self.dimen.w, self.dimen.h
    local margin = Theme.space.lg
    local content_w = screen_w - 2 * margin
    local gap = Theme.space.md
    local power = Device:hasBattery() and Device:getPowerDevice():getCapacity() or nil

    -- Rows the focus manager moves through, top to bottom.
    self.selected = { x = 1, y = 1 }
    self.layout = {}

    local top = VerticalGroup:new{ align = "left" }
    table.insert(top, Header:new{
        title = self.book and self.book.title or _("Notes"),
        width = content_w,
        battery_percent = power,
        back_callback = self.book and function() self:showBooks() end or nil,
    })
    table.insert(top, VerticalSpan:new{ width = gap })

    local navbar = NavBar:new{
        width = screen_w,
        active = "notes",
        callbacks = {
            home = function() self:openHome() end,
            library = function() self:openLibrary() end,
            notes = function() self:showBooks() end,
            settings = function() self:openSettings() end,
        },
    }
    local nav_h = navbar:getSize().h

    -- Space for the list: the screen minus the top block, the nav bar and the top padding.
    local items = self:listItems(content_w)
    -- The pager takes its height plus a gap above and below.
    local footer_h = Theme.touch_target + 2 * gap
    local avail = screen_h - gap - top:getSize().h - nav_h
    local pages = paginate(items, avail)
    if #pages > 1 then
        pages = paginate(items, avail - footer_h)
    end
    self.page = math.min(math.max(1, self.page), math.max(1, #pages))

    local body = VerticalGroup:new{ align = "left" }
    if #items == 0 then
        table.insert(body, VerticalSpan:new{ width = gap })
        table.insert(body, TextBoxWidget:new{
            text = _("No highlights or notes yet. Long-press text while reading to highlight it."),
            face = Theme:font("body"),
            fgcolor = Theme.color.ink,
            width = content_w,
        })
    end
    for _k, item in ipairs(pages[self.page] or {}) do
        local widget = item.build()
        table.insert(body, widget)
        if item.focusable then
            table.insert(self.layout, { widget })
        end
    end

    local upper = VerticalGroup:new{ align = "left", top, body }
    -- The pager sits above the nav bar, not under the list.
    local footer
    if #pages > 1 then
        local btn_w = math.floor(content_w * 0.3)
        local prev = PageButton:new{
            text = _("Previous"),
            width = btn_w,
            enabled = self.page > 1,
            callback = function() self:turnPage(-1) end,
        }
        local next_button = PageButton:new{
            text = _("Next"),
            width = btn_w,
            enabled = self.page < #pages,
            callback = function() self:turnPage(1) end,
        }
        local label_w = content_w - 2 * btn_w - 2 * gap
        local label = TextWidget:new{
            text = T(_("Page %1 of %2"), self.page, #pages),
            face = Theme:font("body"),
            fgcolor = Theme.color.ink,
            max_width = label_w,
        }
        footer = HorizontalGroup:new{
            align = "center",
            prev,
            HorizontalSpan:new{ width = gap },
            CenterContainer:new{
                dimen = Geom:new{ w = label_w, h = Theme.touch_target },
                label,
            },
            HorizontalSpan:new{ width = gap },
            next_button,
        }
        table.insert(self.layout, { prev, next_button })
    end

    for _k, row in ipairs(navbar.layout) do
        table.insert(self.layout, row)
    end

    local padded = FrameContainer:new{
        bordersize = 0,
        padding = 0,
        padding_top = gap,
        padding_left = margin,
        padding_right = margin,
        margin = 0,
        upper,
    }
    local footer_block
    local pager_h = 0
    if footer then
        footer_block = FrameContainer:new{
            bordersize = 0,
            padding = 0,
            padding_left = margin,
            padding_right = margin,
            margin = 0,
            footer,
        }
        pager_h = footer_block:getSize().h
    end
    -- Pager height plus the gaps above and below it.
    local below_h = footer_block and (pager_h + 2 * gap) or 0
    local gap_h = math.max(0, screen_h - padded:getSize().h - below_h - nav_h)
    local page = VerticalGroup:new{
        align = "left",
        padded,
        VerticalSpan:new{ width = gap_h },
    }
    if footer_block then
        table.insert(page, VerticalSpan:new{ width = gap })
        table.insert(page, footer_block)
        table.insert(page, VerticalSpan:new{ width = gap })
    end
    table.insert(page, navbar)
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

function Notes:onClose()
    if self.book then
        self:showBooks()
    else
        UIManager:close(self)
    end
    return true
end

function Notes:onCloseWidget()
    if current == self then
        current = nil
    end
end

--- Closes Notes and opens the book at the entry's location.
-- The location is the same pair the bookmark browser passes to gotoBookmark.
function Notes:openEntry(file, entry)
    if not file then
        return
    end
    closeKaiaSurfaces(self)
    local location = entry.location or {}
    require("apps/reader/readerui"):showReader(file, nil, nil, nil, function(ui)
        if ui.link then
            ui.link:addCurrentLocationToStack()
        end
        if ui.bookmark then
            ui.bookmark:gotoBookmark(location.page, location.pos0)
        end
    end)
end

--- Closes Notes and Home, then shows Home.
function Notes:openHome()
    closeKaiaSurfaces(self)
    require("kaia/home"):show()
end

--- Closes Notes and Home, which shows the file manager underneath.
function Notes:openLibrary()
    closeKaiaSurfaces(self)
    local FileManager = require("apps/filemanager/filemanager")
    if not FileManager.instance then
        FileManager:showFiles()
    end
end

--- Opens the file manager main menu. Notes stays underneath it.
function Notes:openSettings()
    local FileManager = require("apps/filemanager/filemanager")
    if FileManager.instance and FileManager.instance.menu then
        FileManager.instance.menu:onShowMenu()
    end
end

-- Sticker extension point: NotesData.stickerProvider, filled by the sticker task.

return Notes
