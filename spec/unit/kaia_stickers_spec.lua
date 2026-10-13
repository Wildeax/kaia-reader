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
local STAR = "resources/kaia/stickers/star.png"

local function repoRoot()
    return os.getenv("KAIA_REPO_ROOT") or "../.."
end

describe("kaia stickers", function()
    local DataStorage, DocumentRegistry, ReaderUI, UIManager, Screen, Geom
    local NotesData, RustLib, Store, util, lfs, ffi
    local lib_path
    local base_dir
    local store_dir
    local original_default_dir
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

    local function addStar(readerui, caption)
        local pos0, pos1 = selectionAtTop(readerui)
        local record, msg = readerui.kaiastickers:addStickerAt(pos0, pos1, caption or "", STAR)
        assert(record, msg)
        return record
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
        original_default_dir = Store.defaultDir
        Store.defaultDir = function()
            return store_dir
        end
        setenv("KAIA_LIBKAIA", lib_path)
        RustLib._reset()
        NotesData.stickerProvider = nil
    end)

    after_each(function()
        if open_reader then
            closeBook(open_reader)
        end
        Store.defaultDir = original_default_dir
        NotesData.stickerProvider = nil
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
        assert.are.equal(-1.1, record.anchor.offset.dy)
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
end)
