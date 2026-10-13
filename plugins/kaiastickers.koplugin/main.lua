--[[--
Kaia stickers: images placed on the text of a book.

The plugin does nothing unless libkaia loads. Then it adds a Sticker button to
the highlight dialog, a Stickers menu, a view module that paints the stickers
on each page, and a sticker provider for Kaia Notes. Stickers and their assets
live in the Kaia store (frontend/kaia/store.lua), never in the book or its
sidecar.

Anchors: reflowable documents store the CREngine xpointer of the selection
start, the selected text with a little context, and an offset in line
heights. Paged documents store the page and the top-left corner of the
selection as fractions of the unrotated page.

@module koplugin.KaiaStickers
--]]--

local ButtonDialog = require("ui/widget/buttondialog")
local ConfirmBox = require("ui/widget/confirmbox")
local DataStorage = require("datastorage")
local DocSettings = require("docsettings")
local InfoMessage = require("ui/widget/infomessage")
local Layer = require("stickerlayer")
local RustLib = require("kaia/rustlib")
local Store = require("kaia/store")
local Screen = require("device").screen
local UIManager = require("ui/uimanager")
local WidgetContainer = require("ui/widget/container/widgetcontainer")
local lfs = require("libs/libkoreader-lfs")
local logger = require("logger")
local util = require("util")
local T = require("ffi/util").template
local _ = require("gettext")

local DEFAULT_WIDTH = 0.07 -- fraction of the screen width
-- Line heights from the anchor line. A margin sticker is centred on its line.
-- An inline sticker sits just above the line.
local DEFAULT_DY = { margin = 0, inline = -1.1 }
local QUOTE_MAX_CHARS = 1000
local CONTEXT_MAX_CHARS = 32
local CONTEXT_WORDS = 4
local REPAIR_MAX_HITS = 100
local MIN_SIZE, MAX_SIZE = 0.02, 1
local MOVE_STEP = 0.5
local MAX_OFFSET = 50

local BUNDLED_DIR = "resources/kaia/stickers"

-- The store of the open book, or nil. Set while a book is open, so the Notes
-- provider can reuse it instead of opening the database a second time.
local active_store
-- The md5 and file of the open book, or nil. Lets Notes find the open book
-- without ReadHistory.
local active_book

local function nonEmpty(text)
    if type(text) == "string" and text ~= "" then
        return text
    end
    return nil
end

local function clampRange(value, low, high)
    return math.max(low, math.min(high, value))
end

local function clamp01(value)
    if value ~= value then -- NaN
        return 0
    end
    return clampRange(value, 0, 1)
end

-- Collapses runs of whitespace, so text from different sources compares equal.
local function normalizeSpace(text)
    return (text:gsub("%s+", " "):gsub("^ ", ""):gsub(" $", ""))
end

-- Keeps the last n characters of a string, counted in UTF-8 characters.
local function lastChars(text, n)
    local chars = util.splitToChars(text)
    local count = #chars
    if count <= n then
        return text
    end
    return table.concat(chars, "", count - n + 1, count)
end

-- Keeps the first n characters of a string.
local function firstChars(text, n)
    local chars = util.splitToChars(text)
    if #chars <= n then
        return text
    end
    return table.concat(chars, "", 1, n)
end

-- Moves two xpointers out by up to `words` words each side. Returns the
-- widened start and stop, which bound the text around the selection.
local function widen(doc, pos0, pos1, words)
    local start = pos0
    for _i = 1, words do
        local prev = doc:getPrevVisibleWordStart(start)
        if not prev then
            break
        end
        start = prev
    end
    local stop = pos1
    for _i = 1, words do
        local next_end = doc:getNextVisibleWordEnd(stop)
        if not next_end then
            break
        end
        stop = next_end
    end
    return start, stop
end

-- Returns the quote for a selection in a reflowable book: the exact text and up
-- to 32 characters of context on each side. Nil when the selection has no text.
local function quoteAround(doc, pos0, pos1)
    local exact = normalizeSpace(util.cleanupSelectedText(doc:getTextFromXPointers(pos0, pos1) or ""))
    if exact == "" then
        return nil
    end
    exact = firstChars(exact, QUOTE_MAX_CHARS)
    local start, stop = widen(doc, pos0, pos1, CONTEXT_WORDS)
    local prefix = normalizeSpace(doc:getTextFromXPointers(start, pos0) or "")
    local suffix = normalizeSpace(doc:getTextFromXPointers(pos1, stop) or "")
    return {
        exact = exact,
        prefix = lastChars(prefix, CONTEXT_MAX_CHARS),
        suffix = firstChars(suffix, CONTEXT_MAX_CHARS),
    }
end

-- Finds the quote in the book. Returns the xpointer of the best match, or nil.
-- A match whose context is exactly prefix + exact + suffix wins. Otherwise the
-- first occurrence of the exact text is used. The search covers the whole book:
-- origin 0 searches from the view to the end, origin -1 from the start to the view.
local function locateQuote(doc, quote)
    local wanted = normalizeSpace(table.concat(
        { quote.prefix, quote.exact, quote.suffix }, " "))
    local first_exact
    for _i, origin in ipairs({ 0, -1 }) do
        local hits = doc:findText(quote.exact, origin, 0, false, nil, false, REPAIR_MAX_HITS)
        if type(hits) == "table" then
            for _j, hit in ipairs(hits) do
                local start, stop = widen(doc, hit["start"], hit["end"], CONTEXT_WORDS)
                local haystack = normalizeSpace(doc:getTextFromXPointers(start, stop) or "")
                if haystack:find(wanted, 1, true) then
                    return hit["start"]
                end
                if not first_exact and Store.quoteLocate(haystack, quote.exact, quote.prefix, quote.suffix) then
                    first_exact = hit["start"]
                end
            end
        end
    end
    return first_exact
end

-- Default size: 12 percent of the screen width, height from the image shape.
local function defaultSize(asset)
    local w = DEFAULT_WIDTH
    local aspect = asset.height / asset.width
    local h = w * aspect * Screen:getWidth() / Screen:getHeight()
    return { w = w, h = clampRange(h, MIN_SIZE, MAX_SIZE) }
end

-- Scales a sticker size by a factor, keeping its shape.
local function resize(sticker, factor)
    local w = clampRange(sticker.size.w * factor, MIN_SIZE, MAX_SIZE)
    local ratio = w / sticker.size.w
    sticker.size = { w = w, h = clampRange(sticker.size.h * ratio, MIN_SIZE, MAX_SIZE) }
end

-- Moves a reflowable sticker up (negative delta) or down, in line heights.
local function move(sticker, delta)
    local offset = sticker.anchor.offset
    if not offset then
        return
    end
    offset.dy = clampRange(offset.dy + delta, -MAX_OFFSET, MAX_OFFSET)
end

-- Sticker images that the picker offers: bundled ones, then the user's.
local function stickerFiles()
    local files = {}
    local dirs = { BUNDLED_DIR, DataStorage:getDataDir() .. "/stickers" }
    for _, dir in ipairs(dirs) do
        if lfs.attributes(dir, "mode") == "directory" then
            local names = {}
            for name in lfs.dir(dir) do
                if name:match("%.[Pp][Nn][Gg]$") or name:match("%.[Jj][Pp][Ee]?[Gg]$") then
                    names[#names + 1] = name
                end
            end
            table.sort(names)
            for _, name in ipairs(names) do
                files[#files + 1] = {
                    label = (name:gsub("%.%w+$", "")),
                    path = dir .. "/" .. name,
                }
            end
        end
    end
    return files
end

-- Notes integration

local function noteEntry(sticker)
    local anchor = sticker.anchor or {}
    local location = {}
    if anchor.type == "reflowable" then
        location.pos0 = anchor.locator
    elseif anchor.type == "fixed" then
        location.page = anchor.page
    end
    return {
        kind = "sticker",
        text = nonEmpty(sticker.text) or _("Sticker"),
        location = location,
        datetime = sticker.created_at and os.date("%Y-%m-%d %H:%M:%S", sticker.created_at) or nil,
        sticker_id = sticker.id,
        state = sticker.state,
    }
end

-- Returns a map from partial_md5 to book file for the books in ReadHistory
-- that have a sidecar. The sidecar is read, never written.
local function filesByMD5(wanted)
    local found = {}
    if active_book and wanted[active_book.md5] then
        found[active_book.md5] = active_book.file
    end
    local ok, history = pcall(function()
        return require("readhistory").hist
    end)
    if not ok or not history then
        return found
    end
    for _, item in ipairs(history) do
        local file = item.file
        if file and not item.dim and lfs.attributes(file, "mode") == "file" then
            local path = DocSettings:findSidecarFile(file)
            if path then
                local read, stored = pcall(dofile, path)
                local md5 = read and type(stored) == "table" and stored.partial_md5_checksum
                if md5 and wanted[md5] and not found[md5] then
                    found[md5] = file
                end
            end
        end
    end
    return found
end

-- Builds the table that frontend/kaia/notesdata.lua expects from the stickers.
local function collectForNotes(store)
    local result = { by_file = {}, orphaned = {} }
    local all = store:listAll() or {}
    if #all == 0 then
        return result
    end
    local wanted = {}
    for _, sticker in ipairs(all) do
        if sticker.doc and sticker.doc.partial_md5 then
            wanted[sticker.doc.partial_md5] = true
        end
    end
    local files = filesByMD5(wanted)
    for _, sticker in ipairs(all) do
        local md5 = sticker.doc and sticker.doc.partial_md5
        local file = md5 and files[md5]
        if file then
            local entry = noteEntry(sticker)
            if sticker.state == "orphaned" then
                entry.file = file
                table.insert(result.orphaned, entry)
            else
                result.by_file[file] = result.by_file[file] or {}
                table.insert(result.by_file[file], entry)
            end
        end
    end
    return result
end

-- Notes calls this on every refresh. It reuses the open store, or opens one
-- for the duration of the call. Nil means no stickers can be read.
local function stickerProvider()
    local store = active_store
    local owned = false
    if not store then
        local opened = Store.open(Store.defaultDir())
        if not opened then
            return nil
        end
        store = opened
        owned = true
    end
    local ok, result = pcall(collectForNotes, store)
    if owned then
        store:close()
    end
    if not ok then
        logger.warn("kaia stickers: notes provider failed", result)
        return nil
    end
    return result
end

-- Sets the Notes provider when the notes module loads and no other provider is set.
local function installNotesProvider()
    local loaded, NotesData = pcall(require, "kaia/notesdata")
    if not loaded or type(NotesData) ~= "table" then
        return
    end
    if NotesData.stickerProvider == nil then
        NotesData.stickerProvider = stickerProvider
    end
end

local KaiaStickers = WidgetContainer:extend{
    name = "kaiastickers",
    is_doc_only = false,
}

--- Checks the library once. Without libkaia this plugin adds nothing. The
-- reason is logged once by RustLib.load().
function KaiaStickers:init()
    if not RustLib.load() then
        return
    end
    self.enabled = true
    installNotesProvider()
    if not self.document then
        -- File manager instance: Notes only needs the provider.
        return
    end
    self.ui.highlight:addToHighlightDialog("11_sticker", function(this)
        return {
            text = _("Sticker"),
            callback = function()
                local selected = this.selected_text
                if not selected or not selected.pos0 or not selected.pos1 then
                    return
                end
                local pos0, pos1 = selected.pos0, selected.pos1
                this:onClose()
                self:showStickerPicker(pos0, pos1)
            end,
        }
    end)
    self.layer = Layer:new(self)
    self.view:registerViewModule("kaia_stickers", self.layer)
    self.ui.menu:registerToMainMenu(self)
end

-- Opens the store and records the document. Only the reader opens a book.
function KaiaStickers:onReaderReady()
    if not self.enabled or not self.document then
        return
    end
    local store, reason = Store.open(Store.defaultDir())
    if not store then
        logger.warn("kaia stickers: cannot open the store", reason)
        return
    end
    local file = self.document.file
    local md5 = self.ui.doc_settings:readSetting("partial_md5_checksum") or util.partialMD5(file)
    local size = lfs.attributes(file, "size")
    if not md5 or not size then
        store:close()
        logger.warn("kaia stickers: cannot identify the book", file)
        return
    end
    local props = self.document:getProps()
    local title = props and props.title or nil
    local changed, code, msg = store:documentSeen(md5, size, title)
    if changed == nil then
        store:close()
        logger.warn("kaia stickers: documentSeen failed", code, msg)
        return
    end
    self.store = store
    active_store = store
    active_book = { md5 = md5, file = file }
    self.doc_id = { partial_md5 = md5, file_size = size, title = title }
    if changed then
        UIManager:show(InfoMessage:new{
            text = _("This book file changed since the last visit. Sticker positions need review. Use Stickers > Accept sticker positions in the menu once they look right."),
        })
    end
end

function KaiaStickers:onCloseDocument()
    self:closeStore()
end

function KaiaStickers:closeStore()
    if self.layer then
        self.layer:free()
    end
    if self.store then
        self.store:close()
        if active_store == self.store then
            active_store = nil
            active_book = nil
        end
    end
    self.store = nil
    self.doc_id = nil
    self.cache = nil
    self.repair_failed = nil
end

--- Gives the reader's tap handler a chance to open a sticker. Returns false
-- for taps that miss every sticker, so page turns still work.
function KaiaStickers:onTap(_arg, ges)
    if not self.layer or not self.store or not ges or not ges.pos then
        return false
    end
    local id = self.layer:hitTest(ges.pos)
    if not id then
        return false
    end
    self:showStickerMenu(id)
    return true
end

function KaiaStickers:isVisible()
    return G_reader_settings:nilOrTrue("kaia_stickers_visible")
end

--- Returns the stickers of this book, oldest first. Cached until an edit.
function KaiaStickers:stickers()
    if not self.store then
        return {}
    end
    if not self.cache then
        self.cache = self.store:list(self.doc_id.partial_md5) or {}
    end
    return self.cache
end

function KaiaStickers:invalidate()
    self.cache = nil
end

function KaiaStickers:findSticker(id)
    for _, sticker in ipairs(self:stickers()) do
        if sticker.id == id then
            return sticker
        end
    end
    return nil
end

--- Adds a sticker at a selection. `pos0` and `pos1` are the selection ends from
-- ReaderHighlight: xpointers in reflowable books, page positions in paged
-- ones. `caption` may be empty. Returns the stored record, or nil and a message.
function KaiaStickers:addStickerAt(pos0, pos1, caption, asset_path)
    if not self.store then
        return nil, _("Stickers are not available for this book.")
    end
    local asset, code, msg = self.store:addAsset(asset_path, "")
    if not asset then
        logger.warn("kaia stickers: addAsset failed", code, msg)
        return nil, msg
    end
    local anchor, err = self:anchorFor(pos0, pos1)
    if not anchor then
        return nil, err
    end
    local record, code2, msg2 = self.store:put{
        doc = self.doc_id,
        anchor = anchor,
        asset = asset.sha256,
        text = nonEmpty(caption),
        size = defaultSize(asset),
        rotation = 0,
        state = "attached",
    }
    if not record then
        logger.warn("kaia stickers: put failed", code2, msg2)
        return nil, msg2
    end
    self:invalidate()
    UIManager:setDirty(self.view.dialog, "ui")
    return record
end

-- Builds the anchor table for a selection, or returns nil and a message.
function KaiaStickers:anchorFor(pos0, pos1)
    local doc = self.document
    if self.ui.paging then
        local page = pos0.page
        local boxes = doc:getPageBoxesFromPositions(page, pos0, pos1)
        local box = boxes and boxes[1]
        if not box then
            return nil, _("The selection has no text box on this page.")
        end
        local dims = doc:getNativePageDimensions(page)
        return {
            type = "fixed",
            page = page,
            x = clamp01(box.x / dims.w),
            y = clamp01(box.y / dims.h),
        }
    end
    local quote = quoteAround(doc, pos0, pos1)
    if quote == nil then
        return nil, _("The selection is empty.")
    end
    return {
        type = "reflowable",
        locator = pos0,
        quote = quote,
        offset = { dx = 0, dy = DEFAULT_DY.margin },
        placement = "margin",
    }
end

function KaiaStickers:putSticker(sticker)
    local record, code, msg = self.store:put(sticker)
    if not record then
        logger.warn("kaia stickers: put failed", code, msg)
    end
    self:invalidate()
    return record, msg
end

function KaiaStickers:removeSticker(id)
    local ok, code, msg = self.store:delete(id)
    if not ok then
        logger.warn("kaia stickers: delete failed", code, msg)
    end
    self:invalidate()
    return ok, msg
end

--- Removes every sticker of this book. The assets stay until the store runs gc.
function KaiaStickers:removeAll()
    local removed = 0
    for _, sticker in ipairs(self:stickers()) do
        if self:removeSticker(sticker.id) then
            removed = removed + 1
        end
    end
    return removed
end

--- Sets every needs_review sticker back to attached. Returns the count.
function KaiaStickers:acceptPositions()
    local count = 0
    for _, sticker in ipairs(self:stickers()) do
        if sticker.state == "needs_review" then
            if self.store:setState(sticker.id, "attached") then
                count = count + 1
            end
        end
    end
    self:invalidate()
    return count
end

--- Handles a sticker whose locator no longer resolves. Searches the book for
-- the quote and moves the sticker to the best match. Without a match the
-- sticker becomes orphaned. Nothing is deleted. Returns true when it moved.
function KaiaStickers:repairSticker(sticker)
    -- A repair that failed once is not retried this session, so a broken
    -- sticker does not cost a book search on every page.
    self.repair_failed = self.repair_failed or {}
    if self.repair_failed[sticker.id] then
        return false
    end
    local ok, result = pcall(function()
        local found = locateQuote(self.document, sticker.anchor.quote)
        if found then
            sticker.anchor.locator = found
            self:putSticker(sticker)
            return true
        end
        self.store:setState(sticker.id, "orphaned")
        self:invalidate()
        return false
    end)
    if not ok then
        self.repair_failed[sticker.id] = true
        logger.warn("kaia stickers: repair failed", result)
        return false
    end
    return result
end

-- Stickers whose state is not attached are not drawn. Changing a stored
-- sticker keeps the other fields as they are.
function KaiaStickers:showStickerMenu(id)
    local dialog
    local function edit(change)
        local sticker = self:findSticker(id)
        if not sticker then
            return
        end
        change(sticker)
        local _record, msg = self:putSticker(sticker)
        if not _record then
            UIManager:show(InfoMessage:new{ text = msg or _("The sticker could not be saved.") })
            return
        end
        UIManager:setDirty(self.view.dialog, "ui")
    end
    local sticker = self:findSticker(id)
    if not sticker then
        return
    end
    local buttons = {
        {
            { text = _("Bigger"), callback = function() edit(function(s) resize(s, 1.25) end) end },
            { text = _("Smaller"), callback = function() edit(function(s) resize(s, 0.8) end) end },
        },
    }
    if sticker.anchor.type == "reflowable" then
        local in_margin = sticker.anchor.placement ~= "inline"
        table.insert(buttons, {
            { text = _("Move up"), callback = function() edit(function(s) move(s, -MOVE_STEP) end) end },
            { text = _("Move down"), callback = function() edit(function(s) move(s, MOVE_STEP) end) end },
        })
        table.insert(buttons, {
            {
                text = in_margin and _("Place over text") or _("Place in margin"),
                callback = function()
                    edit(function(s)
                        -- Each placement starts from its own default offset.
                        local next_placement = s.anchor.placement == "inline" and "margin" or "inline"
                        s.anchor.placement = next_placement
                        s.anchor.offset.dy = DEFAULT_DY[next_placement]
                    end)
                end,
            },
        })
    end
    table.insert(buttons, {
        {
            text = _("Remove"),
            callback = function()
                self:removeSticker(id)
                UIManager:close(dialog)
                UIManager:setDirty(self.view.dialog, "ui")
            end,
        },
        { text = _("Close"), callback = function() UIManager:close(dialog) end },
    })
    dialog = ButtonDialog:new{
        title = sticker.text or _("Sticker"),
        buttons = buttons,
    }
    UIManager:show(dialog)
end

function KaiaStickers:showStickerPicker(pos0, pos1)
    local files = stickerFiles()
    if #files == 0 then
        UIManager:show(InfoMessage:new{ text = _("No sticker images found.") })
        return
    end
    local picker
    local rows, row = {}, nil
    for i, file in ipairs(files) do
        if (i - 1) % 3 == 0 then
            row = {}
            table.insert(rows, row)
        end
        table.insert(row, {
            text = file.label,
            callback = function()
                UIManager:close(picker)
                local record, msg = self:addStickerAt(pos0, pos1, "", file.path)
                if not record then
                    UIManager:show(InfoMessage:new{ text = msg or _("The sticker could not be added.") })
                end
            end,
        })
    end
    picker = ButtonDialog:new{
        title = _("Choose a sticker"),
        buttons = rows,
    }
    UIManager:show(picker)
end

function KaiaStickers:exportStickers()
    if not self.store then
        return
    end
    local path, code, msg = self.store:export()
    if not path then
        logger.warn("kaia stickers: export failed", code, msg)
        UIManager:show(InfoMessage:new{ text = msg or _("The export failed.") })
        return
    end
    UIManager:show(InfoMessage:new{
        text = T(_("Stickers exported to:\n%1"), path),
    })
end

function KaiaStickers:addToMainMenu(menu_items)
    local function unavailable()
        UIManager:show(InfoMessage:new{ text = _("Stickers are not available for this book.") })
    end
    menu_items.kaia_stickers = {
        text = _("Stickers"),
        sorting_hint = "tools",
        sub_item_table = {
            {
                text = _("Show stickers"),
                keep_menu_open = true,
                checked_func = function()
                    return self:isVisible()
                end,
                callback = function()
                    G_reader_settings:flipNilOrTrue("kaia_stickers_visible")
                    UIManager:setDirty(self.view.dialog, "ui")
                end,
            },
            {
                text = _("Accept sticker positions"),
                callback = function()
                    if not self.store then
                        return unavailable()
                    end
                    local count = self:acceptPositions()
                    UIManager:setDirty(self.view.dialog, "ui")
                    UIManager:show(InfoMessage:new{
                        text = T(_("Accepted %1 sticker positions."), count),
                    })
                end,
            },
            {
                text = _("Export stickers"),
                callback = function()
                    if not self.store then
                        return unavailable()
                    end
                    self:exportStickers()
                end,
            },
            {
                text = _("Remove all stickers in this book"),
                callback = function()
                    if not self.store then
                        return unavailable()
                    end
                    UIManager:show(ConfirmBox:new{
                        text = _("Remove all stickers in this book? This cannot be undone."),
                        ok_text = _("Remove"),
                        ok_callback = function()
                            self:removeAll()
                            UIManager:setDirty(self.view.dialog, "ui")
                        end,
                    })
                end,
            },
        },
    }
end

return KaiaStickers
