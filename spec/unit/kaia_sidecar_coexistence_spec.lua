-- Kaia must not delete or move stock KOReader's sidecar next to a book.
describe("Kaia sidecar coexistence", function()
    local DocSettings, DataStorage, FileManager, BookInfo, UIManager, filemanagerutil, lfs, util, ffiutil
    local book, ko_sidecar_dir, ko_sidecar, dir, paths

    local function writeFile(path, value)
        local f = assert(io.open(path, "wb"))
        f:write(value)
        f:close()
    end

    local function readFile(path)
        local f = assert(io.open(path, "rb"))
        local value = f:read("*a")
        f:close()
        return value
    end

    local function assertKoreaderSidecarIntact()
        assert.are.equal('return { ["percent_finished"] = 0.25, ["koreader_only"] = "keep me" }\n', readFile(ko_sidecar))
        assert.are.equal("old backup", readFile(ko_sidecar .. ".old"))
        assert.are.equal("not really a png", readFile(ko_sidecar_dir .. "/cover.png"))
        assert.are.equal("KOReader metadata", readFile(ko_sidecar_dir .. "/custom_metadata.lua"))
    end

    setup(function()
        require("commonrequire")
        DocSettings = require("docsettings")
        DataStorage = require("datastorage")
        lfs = require("libs/libkoreader-lfs")
        util = require("util")
        ffiutil = require("ffi/util")
        FileManager = require("apps/filemanager/filemanager")
        BookInfo = require("apps/filemanager/filemanagerbookinfo")
        UIManager = require("ui/uimanager")
        filemanagerutil = require("apps/filemanager/filemanagerutil")
    end)

    before_each(function()
        dir = DataStorage:getDataDir() .. "/kaia-coexist-test"
        util.makePath(dir)
        book = dir .. "/book.epub"
        paths = { book }
        ffiutil.copyFile("spec/front/unit/data/juliet.epub", book)
        ko_sidecar_dir = dir .. "/book.sdr"
        util.makePath(ko_sidecar_dir)
        ko_sidecar = ko_sidecar_dir .. "/metadata.epub.lua"
        writeFile(ko_sidecar, 'return { ["percent_finished"] = 0.25, ["koreader_only"] = "keep me" }\n')
        writeFile(ko_sidecar .. ".old", "old backup")
        writeFile(ko_sidecar_dir .. "/cover.png", "not really a png")
        writeFile(ko_sidecar_dir .. "/custom_metadata.lua", "KOReader metadata")
    end)

    after_each(function()
        G_reader_settings:saveSetting("document_metadata_folder", "dir")
        for _, path in ipairs(paths) do
            DocSettings:open(path):purge()
        end
        ffiutil.purgeDir(dir)
        G_reader_settings:delSetting("document_metadata_folder")
    end)

    it("reads the KOReader sidecar, writes its own copy and leaves the original", function()
        G_reader_settings:saveSetting("document_metadata_folder", "dir")
        local ds = DocSettings:open(book)
        assert.are.equal(0.25, ds:readSetting("percent_finished"))
        assert.are.equal("keep me", ds:readSetting("koreader_only"))
        ds:saveSetting("percent_finished", 0.5)
        local written_dir = ds:flush()
        assert.is_not_nil(written_dir)
        assert.are_not.equal(ko_sidecar_dir, written_dir)
        -- KOReader's file is still there and unchanged.
        local original = dofile(ko_sidecar)
        assert.are.equal(0.25, original.percent_finished)
        assert.is_truthy(lfs.attributes(ko_sidecar_dir .. "/cover.png", "mode"))
        -- Kaia reopens its own copy.
        assert.are.equal(0.5, DocSettings:open(book):readSetting("percent_finished"))
    end)

    it("keeps reading its own copy after KOReader writes a newer sidecar", function()
        G_reader_settings:saveSetting("document_metadata_folder", "dir")
        local ds = DocSettings:open(book)
        ds:saveSetting("kaia_only_highlight", true)
        ds:flush()
        -- Stock KOReader saves later, with a newer modification time.
        local f = io.open(ko_sidecar, "w")
        f:write('return { ["percent_finished"] = 0.9 }\n')
        f:close()
        lfs.touch(ko_sidecar, os.time() + 3600, os.time() + 3600)
        local reopened = DocSettings:open(book)
        assert.is_true(reopened:readSetting("kaia_only_highlight"))
        assert.are.equal(0.9, dofile(ko_sidecar).percent_finished)
    end)

    it("copies book-folder metadata during Move book metadata", function()
        G_reader_settings:saveSetting("document_metadata_folder", "dir")
        assert.is_true(DocSettings.isSidecarFileNotInPreferredLocation(book))
        DocSettings.updateLocation(book, book)
        assertKoreaderSidecarIntact()
        assert.are.equal(0.25, DocSettings:open(book):readSetting("percent_finished"))
        assert.is_truthy(lfs.attributes(DocSettings:getSidecarDir(book, "dir") .. "/metadata.epub.lua"))
    end)

    it("keeps the book-folder sidecar when deleting Kaia's book data", function()
        G_reader_settings:saveSetting("document_metadata_folder", "dir")
        DocSettings.updateLocation(book, book)
        os.remove(book)
        DocSettings.updateLocation(book)
        assertKoreaderSidecarIntact()
        assert.is_nil(lfs.attributes(DocSettings:getSidecarDir(book, "dir") .. "/metadata.epub.lua"))
    end)

    it("writes custom files into Kaia's folder", function()
        G_reader_settings:saveSetting("document_metadata_folder", "dir")
        local image = dir .. "/new-cover.png"
        writeFile(image, "Kaia cover")
        assert.is_true(DocSettings:flushCustomCover(book, image))
        local custom = DocSettings.openSettingsFile(nil)
        custom:saveSetting("custom_props", { title = "Kaia" })
        assert.is_true(custom:flushCustomMetadata(book))
        assertKoreaderSidecarIntact()
        local own_dir = DocSettings:getSidecarDir(book, "dir")
        assert.are.equal("Kaia cover", readFile(own_dir .. "/cover.png"))
        assert.are.equal("Kaia", DocSettings.openSettingsFile(own_dir .. "/custom_metadata.lua")
            :readSetting("custom_props").title)
    end)

    it("leaves KOReader's sidecar at the old path when renaming a book", function()
        G_reader_settings:saveSetting("document_metadata_folder", "dir")
        local own = DocSettings:open(book)
        own:saveSetting("kaia_only", "renamed")
        own:flush()
        local renamed = dir .. "/renamed.epub"
        table.insert(paths, renamed)
        local manager = setmetatable({ mv_bin = "/bin/mv", onRefresh = function() end }, { __index = FileManager })
        manager:renameFile(book, "renamed.epub", true)
        assertKoreaderSidecarIntact()
        assert.is_truthy(lfs.attributes(renamed))
        assert.are.equal("renamed", DocSettings:open(renamed):readSetting("kaia_only"))
        local old_dir = DocSettings:getSidecarDir(book, "dir")
        local leftovers = {}
        if lfs.attributes(old_dir) then
            for name in lfs.dir(old_dir) do
                if name ~= "." and name ~= ".." then table.insert(leftovers, name) end
            end
        end
        assert.are.same({}, leftovers)
    end)

    it("leaves KOReader's sidecar at the old path when moving a book", function()
        G_reader_settings:saveSetting("document_metadata_folder", "hash")
        local own = DocSettings:open(book)
        own:saveSetting("kaia_only", "moved")
        own:flush()
        local target = dir .. "/target"
        util.makePath(target)
        local moved = target .. "/book.epub"
        table.insert(paths, moved)
        local manager = setmetatable({ mv_bin = "/bin/mv", onRefresh = function() end }, { __index = FileManager })
        manager:cutFile(book)
        manager:pasteFileFromClipboard(target)
        assertKoreaderSidecarIntact()
        assert.is_truthy(lfs.attributes(moved))
        assert.are.equal("moved", DocSettings:open(moved):readSetting("kaia_only"))
    end)

    it("copies a book-folder-only sidecar when moving a book in hash mode", function()
        G_reader_settings:saveSetting("document_metadata_folder", "hash")
        local target = dir .. "/target"
        util.makePath(target)
        local moved = target .. "/book.epub"
        table.insert(paths, moved)
        local manager = setmetatable({ mv_bin = "/bin/mv", onRefresh = function() end }, { __index = FileManager })
        manager:cutFile(book)
        manager:pasteFileFromClipboard(target)
        assertKoreaderSidecarIntact()
        assert.is_truthy(lfs.attributes(DocSettings:getSidecarDir(moved, "hash") .. "/metadata.epub.lua"))
        assert.are.equal(0.25, DocSettings:open(moved):readSetting("percent_finished"))
    end)

    it("resets only Kaia's copy and explains what remains", function()
        G_reader_settings:saveSetting("document_metadata_folder", "dir")
        local own = DocSettings:open(book)
        own:saveSetting("kaia_only", true)
        own:flush()
        local button = filemanagerutil.genResetSettingsButton(own, function() end)
        local old_show, shown = UIManager.show
        UIManager.show = function(_, widget) shown = widget end
        local ok, err = pcall(function()
            button.callback()
            assert.is_truthy(shown.text:find("Only Kaia's copy will be reset", 1, true))
            shown.ok_callback()
        end)
        UIManager.show = old_show
        assert.is_true(ok, err)
        assertKoreaderSidecarIntact()
        assert.is_nil(lfs.attributes(DocSettings:getSidecarDir(book, "dir") .. "/metadata.epub.lua"))
    end)

    it("keeps KOReader's sidecar in a multiple-book reset", function()
        G_reader_settings:saveSetting("document_metadata_folder", "dir")
        local own = DocSettings:open(book)
        own:saveSetting("kaia_only", true)
        own:flush()
        require("ui/widget/booklist").setBookInfoCacheProperty(book, "been_opened", true)
        local button = filemanagerutil.genMultipleResetSettingsButton({ [book] = true }, function() end)
        local old_show, shown = UIManager.show
        UIManager.show = function(_, widget) shown = widget end
        local ok, err = pcall(function()
            button.callback()
            assert.is_truthy(shown.text:find("Only Kaia's copies will be reset", 1, true))
            shown.ok_callback()
        end)
        UIManager.show = old_show
        assert.is_true(ok, err)
        assertKoreaderSidecarIntact()
        assert.is_nil(lfs.attributes(DocSettings:getSidecarDir(book, "dir") .. "/metadata.epub.lua"))
    end)

    it("keeps KOReader's sidecar when removing empty notebook settings", function()
        G_reader_settings:saveSetting("document_metadata_folder", "dir")
        local own = DocSettings:open(book)
        own:saveSetting("kaia_only", true)
        own:flush()
        local info = setmetatable({
            is_current_doc = false,
            ui = {},
            kvp_widget = { onClose = function() end },
            show = function() end,
        }, { __index = BookInfo })
        local old_show, shown = UIManager.show
        local PathChooser = require("ui/widget/pathchooser")
        local old_new = PathChooser.new
        PathChooser.new = function(_, opts) return opts end
        UIManager.show = function(_, widget) shown = widget end
        local ok, err = pcall(function()
            info:showNotebookFileDialog(book .. ".txt", own)
            shown.buttons[3][1].callback()
            shown.onConfirm(nil)
        end)
        UIManager.show = old_show
        PathChooser.new = old_new
        assert.is_true(ok, err)
        assertKoreaderSidecarIntact()
        assert.is_nil(lfs.attributes(DocSettings:getSidecarDir(book, "dir") .. "/metadata.epub.lua"))
    end)

    it("skips an unparsable book-folder candidate without deleting it", function()
        G_reader_settings:saveSetting("document_metadata_folder", "dir")
        writeFile(ko_sidecar, "not valid Lua\n")
        local own = DocSettings:open(book)
        assert.is_nil(own:readSetting("percent_finished"))
        assert.are.equal("not valid Lua\n", readFile(ko_sidecar))
        assert.are.equal("old backup", readFile(ko_sidecar .. ".old"))
    end)

    it("keeps upstream behavior when the user chooses the book folder", function()
        G_reader_settings:saveSetting("document_metadata_folder", "doc")
        local ds = DocSettings:open(book)
        ds:saveSetting("percent_finished", 0.75)
        assert.are.equal(ko_sidecar_dir, ds:flush())
        assert.are.equal(0.75, dofile(ko_sidecar).percent_finished)
    end)

    it("applies the Kaia default only when unset", function()
        local Defaults = require("kaia/defaults")
        G_reader_settings:delSetting("document_metadata_folder")
        Defaults:apply(G_reader_settings)
        assert.are.equal("dir", G_reader_settings:readSetting("document_metadata_folder"))
        G_reader_settings:saveSetting("document_metadata_folder", "doc")
        Defaults:apply(G_reader_settings)
        assert.are.equal("doc", G_reader_settings:readSetting("document_metadata_folder"))
    end)
end)
