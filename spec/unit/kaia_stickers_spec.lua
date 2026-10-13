--[[--
Tests for the Kaia stickers plugin: plugins/kaiastickers.koplugin.

Before running, build the library for the emulator:

    tools/kaia/build-libkaia.sh host

The spec sets KAIA_LIBKAIA to <repo>/rust/target/release/libkaia.<ext>. The
repo root is KAIA_REPO_ROOT when that variable is set, otherwise ../.. from the
working directory. Each test gets its own store directory by replacing
Store.defaultDir for the duration of the test.
]]

local BOOK = "spec/front/unit/data/juliet.epub"
-- A PDF with a text layer. 2col.pdf has none, so it cannot give selections.
local PDF = "spec/front/unit/data/paper.pdf"
local STAR = "resources/kaia/stickers/star.png"

local function repoRoot()
    return os.getenv("KAIA_REPO_ROOT") or "../.."
end

-- Asserts that two numbers differ by at most `tol`.
local function near(expected, actual, tol, what)
    assert(math.abs(expected - actual) <= tol,
        (what or "value") .. ": expected " .. expected .. ", got " .. actual)
end

describe("kaia stickers", function()
    local DataStorage, DocumentRegistry, ReaderUI, UIManager, Screen, Geom
    local NotesData, RustLib, Store, Theme, util, lfs, ffi, Bidi
    local lib_path
    local base_dir
    local store_dir
    local original_default_dir
    local original_mirrored
    local original_rotation
    local counter = 0

    local function setenv(name, value)
        if value == nil then
            ffi.C.unsetenv(name)
        else
            assert(ffi.C.setenv(name, value, 1) == 0)
        end
    end

    local function removeTree(path)
        local attr = lfs.attributes(path)
        if not attr then
            return
        end
        if attr.mode == "directory" then
            for entry in lfs.dir(path) do
                if entry ~= "." and entry ~= ".." then
                    removeTree(path .. "/" .. entry)
                end
            end
            lfs.rmdir(path)
        else
            os.remove(path)
        end
    end

    -- The reader that is open, so after_each can close it after a failed test.
    local open_reader

    local function openBook()
        open_reader = ReaderUI:new{
            dimen = Screen:getSize(),
            document = DocumentRegistry:openDocument(BOOK),
        }
        return open_reader
    end

    local function openPdf()
        open_reader = ReaderUI:new{
            dimen = Screen:getSize(),
            document = DocumentRegistry:openDocument(PDF),
        }
        return open_reader
    end

    local function closeBook(readerui)
        readerui:onClose()
        if open_reader == readerui then
            open_reader = nil
        end
    end

    -- A selection of a few words of text on the screen. The view may start on
    -- an image, so the first word comes from a point on the screen that has
    -- text under it. Returns the selection ends as xpointers.
    local function selectionAtTop(readerui, words)
        local doc = readerui.document
        -- The first screen of this book is a cover image. Move to a page of text.
        readerui.rolling:onGotoPercent(20)
        -- The view must render before the engine can report words on screen.
        readerui.view:paintTo(Screen.bb, 0, 0)
        local pos0
        for y = 60, Screen:getHeight() - 60, 40 do
            local word = doc:getNearestWordFromPosition({ x = Screen:getWidth() / 2, y = y }, 0)
            if word and word.pos0 and #doc:getScreenBoxesFromPositions(word.pos0, word.pos1, true) > 0 then
                pos0 = word.pos0
                break
            end
        end
        assert(pos0, "no word on the screen")
        local pos1 = pos0
        for _i = 1, words or 6 do
            pos1 = doc:getNextVisibleWordEnd(pos1) or pos1
        end
        return pos0, pos1
    end

    -- A line of text on the screen of a paged book, as a selection. Returns the
    -- two positions, which carry the page, and the selected text.
    local function pdfSelection(readerui)
        readerui.view:paintTo(Screen.bb, 0, 0)
        local view = readerui.view
        for y = 40, Screen:getHeight() - 40, 30 do
            local a = view:screenToPageTransform({ x = 80, y = y })
            local b = view:screenToPageTransform({ x = Screen:getWidth() - 80, y = y + 10 })
            local res = readerui.document:getTextFromPositions(a, b)
            if res and res.text and res.text ~= "" then
                return res.pos0, res.pos1, res.text
            end
        end
        error("no text on the screen")
    end

    local function addStar(readerui, caption)
        local pos0, pos1 = selectionAtTop(readerui)
        local record, msg = readerui.kaiastickers:addStickerAt(pos0, pos1, caption or "", STAR)
        assert(record, msg)
        return record
    end

    -- Sets the placement of a sticker, the way the tap dialog does, and saves it.
    local function setPlacement(readerui, id, placement)
        local sticker = assert(readerui.kaiastickers:findSticker(id))
        sticker.anchor.placement = placement
        sticker.anchor.offset.dy = placement == "inline" and -1.1 or 0
        local saved = assert(readerui.kaiastickers:putSticker(sticker))
        return saved
    end

    local function layerOf(readerui)
        return readerui.view.view_modules.kaia_stickers
    end

    -- Paints the whole view, as the reader does, and returns the sticker ids
    -- that the stickers layer drew. The page must be rendered first, so the
    -- layer paints through ReaderView rather than on its own.
    local function paintedIds(readerui)
        local layer = layerOf(readerui)
        readerui.view:paintTo(Screen.bb, 0, 0)
        local ids = {}
        for _i, hit in ipairs(layer.hits) do
            ids[hit.id] = true
        end
        return ids
    end

    -- Paints the view and returns the drawn rectangle of one sticker, or nil.
    local function hitOf(readerui, id)
        paintedIds(readerui)
        for _i, hit in ipairs(layerOf(readerui).hits) do
            if hit.id == id then
                return hit
            end
        end
        return nil
    end

    -- Screen point of a native page point, the way highlights are mapped.
    local function pageToScreen(readerui, page, x, y)
        return readerui.view:pageToScreenTransform(page, Geom:new{ x = x, y = y, w = 1, h = 1 })
    end

    -- Runs the highlight dialog button with this key on a selection, the way
    -- a tap on the button does. Returns the button table.
    local function tapHighlightButton(readerui, key, pos0, pos1)
        local highlight = readerui.highlight
        local button = assert(highlight._highlight_buttons[key])(highlight)
        highlight.selected_text = { pos0 = pos0, pos1 = pos1 }
        local original_close = highlight.onClose
        highlight.onClose = function() end
        button.callback()
        highlight.onClose = original_close
        return button
    end

    -- Runs fn and returns the first widget it shows that `accept` takes (by
    -- default, a widget with buttons), and the labels of its buttons.
    local function shownFrom(fn, accept)
        accept = accept or function(widget)
            return widget.buttons ~= nil
        end
        local shown
        local original_show = UIManager.show
        UIManager.show = function(self, widget, ...)
            if not shown and accept(widget) then
                shown = widget
            end
            return original_show(self, widget, ...)
        end
        local ok, err = pcall(fn)
        UIManager.show = original_show
        assert(ok, err)
        assert(shown, "no widget shown")
        local labels = {}
        for _i, row in ipairs(shown.buttons or shown.buttons_table or {}) do
            for _j, button in ipairs(row) do
                labels[#labels + 1] = button.text
            end
        end
        return shown, labels
    end

    local function dialogFrom(fn)
        return shownFrom(fn)
    end

    setup(function()
        require("commonrequire")
        require("ffi/posix_h")
        disable_plugins()
        require("document/canvascontext"):init(require("device"))
        ffi = require("ffi")
        lfs = require("libs/libkoreader-lfs")
        util = require("util")
        DataStorage = require("datastorage")
        DocumentRegistry = require("document/documentregistry")
        Geom = require("ui/geometry")
        ReaderUI = require("apps/reader/readerui")
        Screen = require("device").screen
        UIManager = require("ui/uimanager")
        load_plugin("kaiastickers.koplugin")

        base_dir = DataStorage:getDataDir() .. "/kaia_stickers_spec"
        lfs.mkdir(base_dir)
        local ext = ffi.os == "OSX" and "dylib" or "so"
        lib_path = repoRoot() .. "/rust/target/release/libkaia." .. ext
        assert(lfs.attributes(lib_path), "missing " .. lib_path .. ", run tools/kaia/build-libkaia.sh host first")
    end)

    before_each(function()
        counter = counter + 1
        store_dir = base_dir .. "/store-" .. counter
        removeTree(store_dir)
        Store = require("kaia/store")
        RustLib = require("kaia/rustlib")
        NotesData = require("kaia/notesdata")
        Theme = require("kaia/theme")
        Bidi = require("ui/bidi")
        original_mirrored = Bidi._mirrored_ui_layout
        original_rotation = Screen:getRotationMode()
        original_default_dir = Store.defaultDir
        Store.defaultDir = function()
            return store_dir
        end
        setenv("KAIA_LIBKAIA", lib_path)
        RustLib._reset()
        NotesData.stickerProvider = nil
        NotesData.stickerActions = nil
    end)

    after_each(function()
        if open_reader then
            closeBook(open_reader)
        end
        if Screen:getRotationMode() ~= original_rotation then
            Screen:setRotationMode(original_rotation)
        end
        Store.defaultDir = original_default_dir
        NotesData.stickerProvider = nil
        NotesData.stickerActions = nil
        Bidi._mirrored_ui_layout = original_mirrored
    end)

    teardown(function()
        removeTree(base_dir)
        setenv("KAIA_LIBKAIA", nil)
        RustLib._reset()
    end)

    it("stays inert when libkaia is missing", function()
        setenv("KAIA_LIBKAIA", base_dir .. "/no-such-libkaia.so")
        RustLib._reset()
        local readerui = openBook()
        assert.is_nil(readerui.kaiastickers.enabled)
        assert.is_nil(readerui.kaiastickers.store)
        assert.is_nil(readerui.highlight._highlight_buttons["11_sticker"])
        assert.is_nil(layerOf(readerui))
        assert.is_nil(NotesData.stickerProvider)
        assert.is_nil(NotesData.stickerActions)
        assert.is_nil(lfs.attributes(store_dir))
        closeBook(readerui)
    end)

    it("adds a sticker and keeps it across a reopen", function()
        local readerui = openBook()
        local pos0, pos1 = selectionAtTop(readerui)
        local record = assert(readerui.kaiastickers:addStickerAt(pos0, pos1, "a star", STAR))
        assert.is_string(record.id)
        assert.are.equal("attached", record.state)
        assert.are.equal("reflowable", record.anchor.type)
        assert.are.equal(pos0, record.anchor.locator)
        assert.is_true(#record.anchor.quote.exact > 0)
        assert.are.equal("margin", record.anchor.placement)
        assert.are.equal(0, record.anchor.offset.dy)
        assert.are.equal("a star", record.text)
        closeBook(readerui)

        readerui = openBook()
        local stickers = readerui.kaiastickers:stickers()
        assert.are.equal(1, #stickers)
        assert.are.equal(record.id, stickers[1].id)
        assert.are.equal("attached", stickers[1].state)
        closeBook(readerui)
    end)

    it("draws the sticker on the page and opens its menu on a tap", function()
        local readerui = openBook()
        local record = addStar(readerui)
        local ids = paintedIds(readerui)
        assert.is_true(ids[record.id] == true)

        local hit = layerOf(readerui).hits[1]
        assert.is_true(readerui.kaiastickers:onTap(nil, { pos = Geom:new{ x = hit.x + 1, y = hit.y + 1 } }))
        -- A tap away from the sticker is not handled, so page turns still work.
        assert.is_false(readerui.kaiastickers:onTap(nil, {
            pos = Geom:new{ x = Screen:getWidth() - 2, y = Screen:getHeight() - 2 },
        }))
        UIManager:close(UIManager:getTopmostVisibleWidget())
        closeBook(readerui)
    end)

    it("keeps the sticker attached after a font size change", function()
        local readerui = openBook()
        local record = addStar(readerui)
        readerui.font:onSetFontSize(readerui.font.configurable.font_size + 6)
        local ids = paintedIds(readerui)
        local stored = readerui.kaiastickers.store:get(record.id)
        assert.are.equal("attached", stored.state)
        assert.is_true(ids[record.id] == true)
        readerui.font:onSetFontSize(readerui.font.configurable.font_size - 6)
        assert.are.equal("attached", readerui.kaiastickers.store:get(record.id).state)
        closeBook(readerui)
    end)

    it("keeps the placement across a reopen", function()
        local readerui = openBook()
        local record = addStar(readerui)
        setPlacement(readerui, record.id, "inline")
        closeBook(readerui)

        readerui = openBook()
        local stored = assert(readerui.kaiastickers:findSticker(record.id))
        assert.are.equal("inline", stored.anchor.placement)
        assert.are.equal(-1.1, stored.anchor.offset.dy)
        setPlacement(readerui, record.id, "margin")
        closeBook(readerui)

        readerui = openBook()
        stored = assert(readerui.kaiastickers:findSticker(record.id))
        assert.are.equal("margin", stored.anchor.placement)
        closeBook(readerui)
    end)

    it("draws a margin sticker outside the text column", function()
        local readerui = openBook()
        addStar(readerui)
        local ids = paintedIds(readerui)
        local layer = layerOf(readerui)
        assert.is_true(next(ids) ~= nil)
        local hit = layer.hits[1]
        local _, text_right = layer:textColumn()
        assert.is_true(hit.x >= text_right, "sticker starts at " .. hit.x .. ", text ends at " .. text_right)
        assert.is_true(hit.x + hit.w <= Screen:getWidth())
        assert.is_true(hit.w > 0 and hit.h > 0)
        closeBook(readerui)
    end)

    it("draws a margin sticker on the left side when the layout is right-to-left", function()
        Bidi._mirrored_ui_layout = true
        local readerui = openBook()
        addStar(readerui)
        paintedIds(readerui)
        local layer = layerOf(readerui)
        local text_left = layer:textColumn()
        local hit = layer.hits[1]
        assert.is_true(hit.x + hit.w <= text_left, "sticker ends at " .. (hit.x + hit.w) .. ", text starts at " .. text_left)
        assert.is_true(hit.x >= 0)
        closeBook(readerui)
    end)

    it("draws over the end of the line, faded, when the margin is too narrow", function()
        local readerui = openBook()
        addStar(readerui)
        local layer = layerOf(readerui)
        -- Stub the measured text column so the room beside it is about 10 px.
        layer.textColumn = function()
            return 0, Screen:getWidth() - 10
        end
        paintedIds(readerui)
        local hit = assert(layer.hits[1], "sticker not drawn")
        local _, text_right = layer:textColumn()
        assert.is_true(hit.x < text_right, "faded sticker should sit over the text")
        assert.is_true(hit.x + hit.w <= Screen:getWidth())
        local faded = false
        for key in pairs(layer.images) do
            if key:find(":faded", 1, true) then
                faded = true
            end
        end
        assert.is_true(faded)
        closeBook(readerui)
    end)

    it("draws an inline sticker at its own size over the text", function()
        local readerui = openBook()
        local record = addStar(readerui)
        setPlacement(readerui, record.id, "inline")
        paintedIds(readerui)
        local layer = layerOf(readerui)
        local hit = assert(layer.hits[1], "sticker not drawn")
        assert.are.equal(math.floor(record.size.w * Screen:getWidth() + 0.5), hit.w)
        closeBook(readerui)
    end)

    it("removes one sticker or all of them", function()
        local readerui = openBook()
        local first = addStar(readerui, "one")
        addStar(readerui, "two")
        readerui.kaiastickers:removeSticker(first.id)
        assert.are.equal(1, #readerui.kaiastickers:stickers())
        assert.are.equal(1, readerui.kaiastickers:removeAll())
        assert.are.equal(0, #readerui.kaiastickers:stickers())
        closeBook(readerui)

        readerui = openBook()
        assert.are.equal(0, #readerui.kaiastickers:stickers())
        closeBook(readerui)
    end)

    it("flags stickers for review when the file size changed", function()
        local readerui = openBook()
        addStar(readerui)
        closeBook(readerui)

        local md5 = util.partialMD5(BOOK)
        local size = lfs.attributes(BOOK, "size")
        local store = assert(Store.open(store_dir))
        assert.is_true(store:documentSeen(md5, size + 1, "Juliet"))
        store:close()

        readerui = openBook()
        local stickers = readerui.kaiastickers:stickers()
        assert.are.equal("needs_review", stickers[1].state)
        assert.is_nil(next(paintedIds(readerui)))
        assert.are.equal(1, readerui.kaiastickers:acceptPositions())
        assert.are.equal("attached", readerui.kaiastickers:stickers()[1].state)
        closeBook(readerui)
    end)

    it("re-anchors a sticker whose locator no longer resolves", function()
        local readerui = openBook()
        local record = addStar(readerui)
        local broken = readerui.kaiastickers.store:get(record.id)
        broken.anchor.locator = "/body/no-such-node"
        assert(readerui.kaiastickers.store:put(broken))
        readerui.kaiastickers:invalidate()

        local ids = paintedIds(readerui)
        local repaired = readerui.kaiastickers.store:get(record.id)
        assert.are.equal("attached", repaired.state)
        assert.are_not.equal("/body/no-such-node", repaired.anchor.locator)
        assert.is_true(ids[record.id] == true)
        closeBook(readerui)
    end)

    it("orphans a sticker whose quote is not in the book", function()
        local readerui = openBook()
        local store = readerui.kaiastickers.store
        local asset = assert(store:addAsset(STAR, ""))
        local record = assert(store:put{
            doc = readerui.kaiastickers.doc_id,
            anchor = {
                type = "reflowable",
                locator = "/body/no-such-node",
                quote = { exact = "text that is not in this book", prefix = "", suffix = "" },
                offset = { dx = 0, dy = -1.1 },
            },
            asset = asset.sha256,
            size = { w = 0.1, h = 0.1 },
            rotation = 0,
            state = "attached",
        })
        readerui.kaiastickers:invalidate()
        paintedIds(readerui)
        assert.are.equal("orphaned", store:get(record.id).state)
        closeBook(readerui)
    end)

    it("gives sticker entries to Notes, with orphans listed apart", function()
        local readerui = openBook()
        addStar(readerui, "a star")
        assert.is_function(NotesData.stickerProvider)
        local data = NotesData.stickers()
        assert.is_table(data)
        local entries = data.by_file[BOOK]
        assert.are.equal(1, #entries)
        assert.are.equal("sticker", entries[1].kind)
        assert.are.equal("a star", entries[1].text)
        assert.is_string(entries[1].location.pos0)
        assert.are.equal(0, #data.orphaned)
        closeBook(readerui)
    end)

    -- Text-only stickers

    it("draws a text note in the margin when there is room, and keeps it across a reopen", function()
        local readerui = openBook()
        local pos0, pos1 = selectionAtTop(readerui)
        local record = assert(readerui.kaiastickers:addTextNoteAt(pos0, pos1, "Check the ending"))
        assert.is_nil(record.asset)
        assert.are.equal("Check the ending", record.text)
        assert.are.equal("attached", record.state)
        -- Stub the text column so the room beside it is wide enough for a note.
        local layer = layerOf(readerui)
        layer.textColumn = function()
            return 0, Screen:getWidth() - 300
        end
        local hit = assert(hitOf(readerui, record.id), "text note not drawn")
        local _, text_right = layer:textColumn()
        assert.is_true(hit.x >= text_right, "note starts at " .. hit.x .. ", text ends at " .. text_right)
        assert.is_true(hit.w > 0 and hit.h > 0)
        closeBook(readerui)

        readerui = openBook()
        local stored = assert(readerui.kaiastickers:findSticker(record.id))
        assert.is_nil(stored.asset)
        assert.are.equal("Check the ending", stored.text)
        assert.is_true(hitOf(readerui, record.id) ~= nil)
        closeBook(readerui)
    end)

    it("draws a marker in the margin when the margin is too narrow for a box", function()
        local readerui = openBook()
        local pos0, pos1 = selectionAtTop(readerui)
        local record = assert(readerui.kaiastickers:addTextNoteAt(pos0, pos1, "Narrow margin"))
        local hit = assert(hitOf(readerui, record.id), "text note not drawn")
        assert.are.equal("marker", hit.kind)
        -- The marker is outside the text column, so no word is covered.
        local _, text_right = layerOf(readerui):textColumn()
        assert.is_true(hit.x >= text_right, "marker starts at " .. hit.x .. ", text ends at " .. text_right)
        assert.is_true(hit.x + hit.w <= Screen:getWidth())
        -- A square between the minimum and maximum marker sizes.
        assert.are.equal(hit.w, hit.h)
        assert.is_true(hit.w >= Screen:scaleBySize(16) and hit.w <= Screen:scaleBySize(28),
            "marker width " .. hit.w)
        closeBook(readerui)
    end)

    it("opens the full text when a marker is tapped", function()
        local readerui = openBook()
        local pos0, pos1 = selectionAtTop(readerui)
        local record = assert(readerui.kaiastickers:addTextNoteAt(pos0, pos1, "The full note text"))
        local hit = assert(hitOf(readerui, record.id))
        local viewer = shownFrom(function()
            assert.is_true(readerui.kaiastickers:onTap(nil, {
                pos = Geom:new{ x = hit.x + 1, y = hit.y + 1 },
            }))
        end, function(widget)
            return widget.buttons_table ~= nil
        end)
        assert.are.equal("The full note text", viewer.text)
        UIManager:close(viewer)
        closeBook(readerui)
    end)

    it("edits a note from its viewer through the text prompt", function()
        local readerui = openBook()
        local pos0, pos1 = selectionAtTop(readerui)
        local record = assert(readerui.kaiastickers:addTextNoteAt(pos0, pos1, "Old words"))
        local viewer, labels = shownFrom(function()
            readerui.kaiastickers:showTextNote(record.id)
        end, function(widget)
            return widget.buttons_table ~= nil
        end)
        assert.are.same({ "Edit", "Remove", "Close" }, labels)
        -- Edit closes the viewer and opens the text prompt, filled with the note.
        local prompt = shownFrom(function()
            viewer.buttons_table[1][1].callback()
        end, function(widget)
            return widget.title == "Edit note"
        end)
        assert.is_not_nil(prompt)
        UIManager:close(prompt)
        assert.are.equal("Old words", readerui.kaiastickers:findSticker(record.id).text)

        local edited = assert(readerui.kaiastickers:editText(record.id, "New words"))
        assert.are.equal("New words", edited.text)
        assert.is_nil(edited.asset)
        assert.is_nil(readerui.kaiastickers:editText(record.id, ""))
        closeBook(readerui)
    end)

    it("keeps a text note in its box when the margin has room", function()
        local readerui = openBook()
        local pos0, pos1 = selectionAtTop(readerui)
        local record = assert(readerui.kaiastickers:addTextNoteAt(pos0, pos1, "Roomy margin"))
        local layer = layerOf(readerui)
        layer.textColumn = function()
            return 0, Screen:getWidth() - 300
        end
        local hit = assert(hitOf(readerui, record.id))
        assert.are.equal("sticker", hit.kind)
        assert.is_true(hit.w > Screen:scaleBySize(28), "a box is wider than a marker")
        closeBook(readerui)
    end)

    it("sizes a text note to its text, with at most three lines", function()
        local readerui = openBook()
        local layer = layerOf(readerui)
        local face = Theme:font("caption")
        local line_h = math.floor((1 + 0.3) * face.size + 0.5)
        local frame_chrome = 2 * (Theme.space.xs + Theme.line.divider)
        local _, short_h = layer:note({ id = "short", text = "Short" }, 200)
        local _, long_h = layer:note({ id = "long", text = string.rep("a long remark about the text ", 40) }, 200)
        assert.is_true(short_h < long_h, "a longer note should be taller")
        assert.is_true(long_h <= 3 * line_h + frame_chrome + 1, "note is taller than three lines: " .. long_h)
        assert.is_true(long_h > 2 * line_h, "note should use its lines")
        closeBook(readerui)
    end)

    it("shows a long text note in Notes as a short preview", function()
        local readerui = openBook()
        local pos0, pos1 = selectionAtTop(readerui)
        local text = string.rep("word ", 60)
        assert(readerui.kaiastickers:addTextNoteAt(pos0, pos1, text))
        local entries = NotesData.stickers().by_file[BOOK]
        assert.are.equal(1, #entries)
        assert.is_true(#entries[1].text < #text, "the Notes entry should be shortened")
        assert.is_true(#entries[1].text > 0)
        closeBook(readerui)
    end)

    -- Orphan repair

    it("places an orphaned sticker at a new selection and keeps its id", function()
        local readerui = openBook()
        local record = addStar(readerui, "moved")
        readerui.kaiastickers.store:setState(record.id, "orphaned")
        readerui.kaiastickers:invalidate()
        assert.is_nil(next(paintedIds(readerui)), "an orphan is not drawn")

        assert.is_true(readerui.kaiastickers:beginPlacement(record.id))
        assert.are.equal(record.id, readerui.kaiastickers.placing_id)

        local pos0, pos1 = selectionAtTop(readerui, 3)
        tapHighlightButton(readerui, "11_sticker", pos0, pos1)

        local stickers = readerui.kaiastickers:stickers()
        assert.are.equal(1, #stickers, "placing must not add a sticker")
        assert.are.equal(record.id, stickers[1].id)
        assert.are.equal("attached", stickers[1].state)
        assert.are.equal(pos0, stickers[1].anchor.locator)
        assert.is_nil(readerui.kaiastickers.placing_id)
        assert.is_true(hitOf(readerui, record.id) ~= nil)
        closeBook(readerui)

        readerui = openBook()
        local stored = assert(readerui.kaiastickers:findSticker(record.id))
        assert.are.equal("attached", stored.state)
        assert.are.equal("moved", stored.text)
        closeBook(readerui)
    end)

    it("cancels placement without changing the sticker", function()
        local readerui = openBook()
        local record = addStar(readerui)
        readerui.kaiastickers.store:setState(record.id, "orphaned")
        readerui.kaiastickers:invalidate()
        local cancel = readerui.highlight._highlight_buttons["11_sticker_cancel"](readerui.highlight)
        assert.is_false(cancel.show_in_highlight_dialog_func())

        readerui.kaiastickers:beginPlacement(record.id)
        assert.is_true(cancel.show_in_highlight_dialog_func())
        tapHighlightButton(readerui, "11_sticker_cancel")
        assert.is_nil(readerui.kaiastickers.placing_id)
        assert.are.equal("orphaned", readerui.kaiastickers:findSticker(record.id).state)
        closeBook(readerui)
    end)

    it("does not place when the sticker is not in the book", function()
        local readerui = openBook()
        assert.is_false(readerui.kaiastickers:beginPlacement("00000000-0000-4000-8000-000000000000"))
        assert.is_nil(readerui.kaiastickers.placing_id)
        closeBook(readerui)
    end)

    it("starts placement from Notes through the stickerActions table", function()
        local readerui = openBook()
        local record = addStar(readerui)
        assert.is_table(NotesData.stickerActions)
        NotesData.stickerActions.place(readerui, record.id)
        assert.are.equal(record.id, readerui.kaiastickers.placing_id)
        closeBook(readerui)
    end)

    it("offers place and remove for an orphan, and accept and remove for a review", function()
        local readerui = openBook()
        local record = addStar(readerui, "row")
        readerui.kaiastickers.store:setState(record.id, "orphaned")
        readerui.kaiastickers:invalidate()
        local notes = require("kaia/notes"):show({ history = {} })
        local orphan = assert(NotesData.stickers().orphaned[1])
        local dialog, labels = dialogFrom(function()
            notes:openSticker(orphan.file, orphan)
        end)
        assert.are.same({ "Place sticker", "Remove sticker", "Close" }, labels)
        UIManager:close(dialog)

        local review = { kind = "sticker", text = "row", state = "needs_review", sticker_id = record.id }
        dialog, labels = dialogFrom(function()
            notes:openSticker(BOOK, review)
        end)
        assert.are.same({ "Accept position", "Open book", "Remove sticker", "Close" }, labels)
        UIManager:close(dialog)
        UIManager:close(notes)
        closeBook(readerui)
    end)

    it("accepts and removes stickers from Notes, with the book open", function()
        local readerui = openBook()
        local record = addStar(readerui)
        readerui.kaiastickers.store:setState(record.id, "needs_review")
        assert.is_true(NotesData.stickerActions.accept(record.id))
        assert.are.equal("attached", readerui.kaiastickers:findSticker(record.id).state)
        assert.is_true(NotesData.stickerActions.remove(record.id))
        assert.are.equal(0, #readerui.kaiastickers:stickers())
        closeBook(readerui)
    end)

    it("accepts and removes stickers from Notes, with no book open", function()
        local readerui = openBook()
        local record = addStar(readerui)
        readerui.kaiastickers.store:setState(record.id, "needs_review")
        closeBook(readerui)

        assert.is_true(NotesData.stickerActions.accept(record.id))
        local store = assert(Store.open(store_dir))
        assert.are.equal("attached", store:get(record.id).state)
        store:close()
        assert.is_true(NotesData.stickerActions.remove(record.id))
        store = assert(Store.open(store_dir))
        assert.is_nil(store:get(record.id))
        store:close()
    end)

    -- Paged documents

    it("adds a fixed sticker from a selection and keeps its page and point across a reopen", function()
        local readerui = openPdf()
        local pos0, pos1 = pdfSelection(readerui)
        local record = assert(readerui.kaiastickers:addStickerAt(pos0, pos1, "", STAR))
        local anchor = record.anchor
        assert.are.equal("fixed", anchor.type)
        assert.are.equal(pos0.page, anchor.page)

        local dims = readerui.document:getNativePageDimensions(anchor.page)
        local box = assert(readerui.document:getPageBoxesFromPositions(anchor.page, pos0, pos1))[1]
        near(box.x / dims.w, anchor.x, 1e-4, "x")
        near(box.y / dims.h, anchor.y, 1e-4, "y")
        assert.is_true(anchor.x >= 0 and anchor.x <= 1 and anchor.y >= 0 and anchor.y <= 1)
        closeBook(readerui)

        readerui = openPdf()
        local stored = assert(readerui.kaiastickers:findSticker(record.id))
        assert.are.equal("fixed", stored.anchor.type)
        assert.are.equal(anchor.page, stored.anchor.page)
        assert.are.equal(anchor.x, stored.anchor.x)
        assert.are.equal(anchor.y, stored.anchor.y)
        closeBook(readerui)
    end)

    it("draws a fixed sticker at its selection and scales it with the zoom", function()
        local readerui = openPdf()
        local pos0, pos1 = pdfSelection(readerui)
        local record = assert(readerui.kaiastickers:addStickerAt(pos0, pos1, "", STAR))
        local anchor = record.anchor
        local dims = readerui.document:getNativePageDimensions(anchor.page)

        local zooms = {}
        for _i, mode in ipairs({ "pagewidth", "pageheight" }) do
            readerui.zooming:setZoomMode(mode)
            local hit = assert(hitOf(readerui, record.id), "sticker not drawn in " .. mode)
            local zoom = readerui.view.state.zoom
            zooms[#zooms + 1] = zoom

            -- The bottom left corner sits on the top left of the selection.
            local corner = assert(pageToScreen(readerui, anchor.page, anchor.x * dims.w, anchor.y * dims.h))
            near(corner.x, hit.x, 1, mode .. " left")
            near(corner.y, hit.y + hit.h, 1, mode .. " bottom")
            -- The size is a fraction of the page on the screen.
            near(record.size.w * dims.w * zoom, hit.w, 1, mode .. " width")
            near(record.size.h * dims.h * zoom, hit.h, 1, mode .. " height")
        end
        assert.are_not.equal(zooms[1], zooms[2], "the two zoom modes should differ")
        closeBook(readerui)
    end)

    it("keeps a fixed sticker on its selection after a rotation change", function()
        local readerui = openPdf()
        local pos0, pos1 = pdfSelection(readerui)
        local record = assert(readerui.kaiastickers:addStickerAt(pos0, pos1, "", STAR))
        local page = record.anchor.page
        local function selectionCorner()
            local box = assert(readerui.document:getPageBoxesFromPositions(page, pos0, pos1))[1]
            return assert(pageToScreen(readerui, page, box.x, box.y))
        end

        local portrait = hitOf(readerui, record.id)
        assert.is_not_nil(portrait)
        local corner = selectionCorner()
        near(corner.x, portrait.x, 1, "portrait left")
        near(corner.y, portrait.y + portrait.h, 1, "portrait bottom")

        readerui.view:onSetRotationMode(1)
        local landscape = assert(hitOf(readerui, record.id), "sticker not drawn in landscape")
        corner = selectionCorner()
        near(corner.x, landscape.x, 1, "landscape left")
        near(corner.y, landscape.y + landscape.h, 1, "landscape bottom")
        closeBook(readerui)
    end)

    it("keeps a fixed sticker on its selection in a cropped view", function()
        local readerui = openPdf()
        local pos0, pos1 = pdfSelection(readerui)
        local record = assert(readerui.kaiastickers:addStickerAt(pos0, pos1, "", STAR))
        local page = record.anchor.page
        readerui.view:onBBoxUpdate(readerui.document:getUsedBBox(page))
        readerui.view:recalculate()
        assert.is_true(readerui.view.use_bbox)

        local hit = assert(hitOf(readerui, record.id), "sticker not drawn in the cropped view")
        local box = assert(readerui.document:getPageBoxesFromPositions(page, pos0, pos1))[1]
        local corner = assert(pageToScreen(readerui, page, box.x, box.y))
        near(corner.x, hit.x, 1, "cropped left")
        near(corner.y, hit.y + hit.h, 1, "cropped bottom")
        readerui.view:onBBoxUpdate(nil)
        closeBook(readerui)
    end)

    it("adds a text note on a paged book", function()
        local readerui = openPdf()
        local pos0, pos1 = pdfSelection(readerui)
        local record = assert(readerui.kaiastickers:addTextNoteAt(pos0, pos1, "Paged note"))
        assert.are.equal("fixed", record.anchor.type)
        local hit = assert(hitOf(readerui, record.id), "paged text note not drawn")
        -- A paged note is a marker beside its line, never a box over the words.
        assert.are.equal("marker", hit.kind)
        assert.is_true(hit.w > 0 and hit.h > 0)
        closeBook(readerui)
    end)
end)
