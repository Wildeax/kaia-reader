describe("kaia library", function()
    local FileManager, Library, Screen

    setup(function()
        require("commonrequire")
        disable_plugins()
        require("document/canvascontext"):init(require("device"))
        FileManager = require("apps/filemanager/filemanager")
        Library = require("kaia/library")
        Screen = require("device").screen
    end)

    local function newFileManager()
        return FileManager:new{
            dimen = Screen:getSize(),
            root_path = "spec/front/unit/data",
        }
    end

    before_each(function()
        G_reader_settings:delSetting(Library.NAVBAR_SETTING)
    end)

    after_each(function()
        G_reader_settings:delSetting(Library.NAVBAR_SETTING)
    end)

    it("attaches the nav bar under the file chooser with Library active", function()
        local filemanager = newFileManager()
        -- The root widget is the chooser container and the nav bar.
        local root = filemanager[1]
        assert.are.equal(2, #root)
        assert.are.equal(filemanager.file_chooser, root[1][1])
        local bar = root[2]
        assert.is_truthy(bar.layout)
        assert.are.equal(4, #bar.layout[1])
        local library_item
        for _, item in ipairs(bar.layout[1]) do
            if item.active then
                library_item = item
            end
        end
        assert.is_truthy(library_item)
        assert.are.equal(library_item, bar.layout[1][2])
        filemanager:onClose()
    end)

    it("shrinks the file chooser by the nav bar height", function()
        local filemanager = newFileManager()
        local bar_h = filemanager[1][2]:getSize().h
        assert.are.equal(Library.navHeight(), bar_h)
        assert.is_true(bar_h > 0)
        assert.are.equal(Screen:getHeight() - bar_h, filemanager.file_chooser.dimen.h)
        filemanager:onClose()
    end)

    it("keeps the full height and no bar when the setting is off", function()
        G_reader_settings:saveSetting(Library.NAVBAR_SETTING, false)
        local filemanager = newFileManager()
        assert.are.equal(0, Library.navHeight())
        assert.are.equal(filemanager.file_chooser, filemanager[1][1])
        assert.are.equal(Screen:getHeight(), filemanager.file_chooser.dimen.h)
        filemanager:onClose()
    end)

    it("turns the bar on by default and off only when the setting is false", function()
        assert.is_true(Library.navbarEnabled())
        G_reader_settings:saveSetting(Library.NAVBAR_SETTING, true)
        assert.is_true(Library.navbarEnabled())
        G_reader_settings:saveSetting(Library.NAVBAR_SETTING, false)
        assert.is_false(Library.navbarEnabled())
    end)

    describe("default display mode", function()
        local bookinfo, coverbrowser, saved_bookinfo

        before_each(function()
            saved_bookinfo = package.loaded["bookinfomanager"]
            bookinfo = { settings = {} }
            function bookinfo:getSetting(key) return self.settings[key] end
            coverbrowser = { calls = {} }
            function coverbrowser:setDisplayMode(mode)
                self.calls[#self.calls + 1] = mode
            end
            package.loaded["bookinfomanager"] = bookinfo
            G_reader_settings:delSetting(Library.DEFAULT_DONE_SETTING)
            G_reader_settings:delSetting("coverbrowser_initial_default_setup_done")
        end)

        after_each(function()
            package.loaded["bookinfomanager"] = saved_bookinfo
            G_reader_settings:delSetting(Library.DEFAULT_DONE_SETTING)
            G_reader_settings:delSetting("coverbrowser_initial_default_setup_done")
        end)

        it("sets the mosaic when no mode was ever chosen", function()
            assert.is_true(Library.applyDefaultDisplayMode({ coverbrowser = coverbrowser }))
            assert.are.same({ Library.DEFAULT_MODE }, coverbrowser.calls)
            -- Runs once.
            assert.is_false(Library.applyDefaultDisplayMode({ coverbrowser = coverbrowser }))
            assert.are.equal(1, #coverbrowser.calls)
        end)

        it("sets the mosaic over the coverbrowser first-run default", function()
            G_reader_settings:makeTrue("coverbrowser_initial_default_setup_done")
            bookinfo.settings.filemanager_display_mode = "list_image_meta"
            bookinfo.settings.history_display_mode = "mosaic_image"
            bookinfo.settings.collection_display_mode = "mosaic_image"
            assert.is_true(Library.applyDefaultDisplayMode({ coverbrowser = coverbrowser }))
            assert.are.same({ Library.DEFAULT_MODE }, coverbrowser.calls)
        end)

        it("keeps a mode the user chose", function()
            G_reader_settings:makeTrue("coverbrowser_initial_default_setup_done")
            bookinfo.settings.filemanager_display_mode = "list_only_meta"
            assert.is_false(Library.applyDefaultDisplayMode({ coverbrowser = coverbrowser }))
            assert.are.same({}, coverbrowser.calls)
        end)

        it("waits for the coverbrowser plugin", function()
            assert.is_false(Library.applyDefaultDisplayMode({}))
            assert.is_nil(G_reader_settings:readSetting(Library.DEFAULT_DONE_SETTING))
        end)
    end)
end)
