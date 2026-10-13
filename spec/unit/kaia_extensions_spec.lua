describe("kaia extensions", function()
    local Extensions, Profile, Blitbuffer, Screen, UIManager
    local real_restart, restart_count

    setup(function()
        require("commonrequire")
        disable_plugins()
        require("document/canvascontext"):init(require("device"))
        Extensions = require("kaia/extensions")
        Profile = require("kaia/compat_profile")
        Blitbuffer = require("ffi/blitbuffer")
        Screen = require("device").screen
        UIManager = require("ui/uimanager")
        real_restart = UIManager.askForRestart
    end)

    before_each(function()
        restart_count = 0
        UIManager.askForRestart = function()
            restart_count = restart_count + 1
        end
        G_reader_settings:delSetting("plugins_disabled")
        G_reader_settings:delSetting("plugins_disable_external")
    end)

    after_each(function()
        UIManager.askForRestart = real_restart
        G_reader_settings:delSetting("plugins_disabled")
        G_reader_settings:delSetting("plugins_disable_external")
    end)

    -- A plugin as the loader holds it after loading: name, path, and the _meta fields.
    local function plugin(name, path, meta)
        local p = {
            name = name,
            path = path or ("plugins/" .. name .. ".koplugin"),
            fullname = name,
            description = "",
        }
        for k, v in pairs(meta or {}) do
            p[k] = v
        end
        return p
    end

    -- A stand-in for PluginLoader with the fields that Extensions reads.
    local function fakeLoader(opts)
        local loader = {
            enabled_plugins = opts.enabled or {},
            disabled_plugins = opts.disabled or {},
            load_errors = opts.errors or {},
            discovered = opts.discovered or {},
            stopped = {},
        }
        function loader:loadPlugins()
            return self.enabled_plugins, self.disabled_plugins
        end
        function loader:_discover()
            return self.discovered
        end
        function loader:stopPluginInstanceByName(name)
            table.insert(self.stopped, name)
            return true
        end
        return loader
    end

    local function collect(loader, opts)
        opts = opts or {}
        return Extensions.collect{
            loader = loader,
            profile = opts.profile or Profile,
            safe_mode = opts.safe_mode,
            disable_external = opts.disable_external,
        }
    end

    local function byName(rows)
        local map = {}
        for _i, row in ipairs(rows) do
            map[row.name] = row
        end
        return map
    end

    -- Every string in the widget tree, so tests can look for visible text.
    local function textsOf(widget)
        local out = {}
        local function walk(w, depth)
            if depth > 25 or type(w) ~= "table" then
                return
            end
            if type(w.text) == "string" then
                table.insert(out, w.text)
            end
            for _i, child in ipairs(w) do
                walk(child, depth + 1)
            end
        end
        walk(widget, 0)
        return out
    end

    local function hasText(widget, text)
        for _i, s in ipairs(textsOf(widget)) do
            if s == text then
                return true
            end
        end
        return false
    end

    local function hasTextPart(widget, part)
        for _i, s in ipairs(textsOf(widget)) do
            if s:find(part, 1, true) then
                return true
            end
        end
        return false
    end

    local function paint(widget)
        local bb = Blitbuffer.new(Screen:getWidth(), Screen:getHeight(), Blitbuffer.TYPE_BB8)
        widget:paintTo(bb, 0, 0)
        bb:free()
    end

    describe("profile", function()
        it("is version 1 for the bundled baseline", function()
            assert.are.equal(1, Profile.version)
            assert.are.equal("KOReader v2026.07.1", Profile.baseline)
        end)

        it("lists bundled and Kaia plugins with class, platform and tested", function()
            local entry = Profile.plugins.archiveviewer
            assert.are.equal("Plugin using shared widgets", entry.class)
            assert.are.equal("applicable", entry.platform)
            assert.is_false(entry.tested)
            assert.are.equal("Kaia", Profile.plugins.kaiastickers.class)
            assert.are.equal("Kaia", Profile.plugins.kaiaexample.class)
        end)

        it("marks no plugin as device tested yet", function()
            for name, entry in pairs(Profile.plugins) do
                assert.is_false(entry.tested, name)
            end
        end)

        it("gives the compatibility text for each kind of entry", function()
            assert.are.equal("Bundled, not yet device tested",
                Extensions.compatibilityText(Profile.plugins.archiveviewer))
            assert.are.equal("Bundled, device tested",
                Extensions.compatibilityText({ class = "Plugin using shared widgets", platform = "applicable", tested = true }))
            assert.are.equal("Kaia", Extensions.compatibilityText(Profile.plugins.kaiastickers))
            assert.are.equal("Not applicable on this device", Extensions.compatibilityText(Profile.plugins.hello))
            assert.are.equal("Not applicable on this device", Extensions.compatibilityText(Profile.plugins.autostandby))
            assert.are.equal("Not in the compatibility profile; works on a best effort basis",
                Extensions.compatibilityText(nil))
        end)
    end)

    describe("origin", function()
        it("treats the app plugin directory as bundled and others as installed", function()
            assert.is_true(Extensions.isBundled("plugins/archiveviewer.koplugin"))
            assert.is_false(Extensions.isBundled("/mnt/onboard/.adds/kaia/plugins/notesx.koplugin"))
            assert.is_false(Extensions.isBundled(nil))
        end)
    end)

    describe("collect", function()
        it("classifies enabled bundled, Kaia and third-party plugins", function()
            local loader = fakeLoader{
                enabled = {
                    plugin("archiveviewer", nil, { fullname = "Archive viewer", description = "Browse archives." }),
                    plugin("kaiastickers", nil, { fullname = "Kaia stickers" }),
                    plugin("notesx", "/data/plugins/notesx.koplugin", { fullname = "Notes extra", version = "1.2" }),
                },
            }
            local rows = byName(collect(loader))

            assert.are.equal("on", rows.archiveviewer.state)
            assert.are.equal("bundled", rows.archiveviewer.origin)
            assert.are.equal("Bundled, not yet device tested", rows.archiveviewer.compat)
            assert.are.equal("Browse archives.", rows.archiveviewer.description)
            assert.is_nil(rows.archiveviewer.version)
            assert.is_true(rows.archiveviewer.can_toggle)

            assert.are.equal("Kaia", rows.kaiastickers.compat)

            assert.are.equal("on", rows.notesx.state)
            assert.are.equal("user", rows.notesx.origin)
            assert.are.equal("1.2", rows.notesx.version)
            assert.are.equal("Not in the compatibility profile; works on a best effort basis", rows.notesx.compat)
            assert.are.equal("/data/plugins/notesx.koplugin", rows.notesx.path)
        end)

        it("classifies a bundled plugin that the user turned off as off and able to turn on", function()
            local loader = fakeLoader{
                disabled = { plugin("texteditor", nil, { fullname = "Text editor" }) },
            }
            local row = byName(collect(loader)).texteditor
            assert.are.equal("off", row.state)
            assert.is_true(row.in_list)
            assert.is_true(row.can_toggle)
            assert.is_nil(row.note)
        end)

        it("classifies third-party plugins as skipped in safe mode", function()
            local loader = fakeLoader{
                enabled = { plugin("archiveviewer", nil, { fullname = "Archive viewer" }) },
                disabled = { plugin("notesx", "/data/plugins/notesx.koplugin", { fullname = "Notes extra" }) },
            }
            local rows = byName(collect(loader, { safe_mode = true }))
            assert.are.equal("skipped", rows.notesx.state)
            assert.is_false(rows.notesx.can_toggle)
            assert.truthy(rows.notesx.note:find("Safe mode", 1, true))
            -- Bundled plugins still run in safe mode.
            assert.are.equal("on", rows.archiveviewer.state)
        end)

        it("keeps a bundled plugin that is off in safe mode as off, not skipped", function()
            local loader = fakeLoader{
                disabled = { plugin("texteditor", nil, { fullname = "Text editor" }) },
            }
            local row = byName(collect(loader, { safe_mode = true })).texteditor
            assert.are.equal("off", row.state)
        end)

        it("classifies a plugin with a load error as failed and keeps the error text", function()
            local loader = fakeLoader{
                discovered = { { name = "broken", path = "/data/plugins/broken.koplugin" } },
                errors = { broken = "/data/plugins/broken.koplugin/main.lua:3: attempt to index a nil value" },
            }
            local row = byName(collect(loader)).broken
            assert.are.equal("failed", row.state)
            assert.are.equal("/data/plugins/broken.koplugin/main.lua:3: attempt to index a nil value", row.error)
            assert.is_false(row.in_list)
            assert.is_false(row.can_toggle)
        end)

        it("shows a discovered plugin that did not load as off, with a note", function()
            local loader = fakeLoader{
                discovered = { { name = "autostandby", path = "plugins/autostandby.koplugin" } },
            }
            local row = byName(collect(loader)).autostandby
            assert.are.equal("off", row.state)
            assert.is_false(row.in_list)
            assert.is_nil(row.error)
            assert.is_false(row.can_toggle)
            assert.truthy(row.note:find("Not loaded", 1, true))
            assert.are.equal("Not applicable on this device", row.compat)
        end)

        it("does not repeat a plugin that is both loaded and discovered", function()
            local loader = fakeLoader{
                enabled = { plugin("archiveviewer", nil) },
                discovered = { { name = "archiveviewer", path = "plugins/archiveviewer.koplugin" } },
            }
            local rows = collect(loader)
            assert.are.equal(1, #rows)
            assert.are.equal("on", rows[1].state)
        end)

        it("blocks turning on third-party plugins while plugins_disable_external is on", function()
            local loader = fakeLoader{
                disabled = {
                    plugin("notesx", "/data/plugins/notesx.koplugin", { fullname = "Notes extra" }),
                    plugin("texteditor", nil, { fullname = "Text editor" }),
                },
            }
            local rows = byName(collect(loader, { disable_external = true }))
            assert.is_false(rows.notesx.can_toggle)
            assert.truthy(rows.notesx.note:find("settings", 1, true))
            assert.is_true(rows.texteditor.can_toggle)
        end)

        it("sorts rows by name", function()
            local loader = fakeLoader{
                enabled = {
                    plugin("zeta", "/data/plugins/zeta.koplugin", { fullname = "zeta" }),
                    plugin("alpha", "/data/plugins/alpha.koplugin", { fullname = "Alpha" }),
                },
            }
            local rows = collect(loader)
            assert.are.equal("alpha", rows[1].name)
            assert.are.equal("zeta", rows[2].name)
        end)
    end)

    describe("setEnabled", function()
        it("turns a plugin off through plugins_disabled, stops it and asks for a restart", function()
            local loader = fakeLoader{}
            Extensions.setEnabled(loader, "texteditor", false)
            assert.is_true(G_reader_settings:readSetting("plugins_disabled").texteditor)
            assert.are.same({ "texteditor" }, loader.stopped)
            assert.are.equal(1, restart_count)
        end)

        it("turns a plugin on by clearing its plugins_disabled entry", function()
            G_reader_settings:saveSetting("plugins_disabled", { texteditor = true })
            local loader = fakeLoader{}
            Extensions.setEnabled(loader, "texteditor", true)
            assert.is_nil(G_reader_settings:readSetting("plugins_disabled").texteditor)
            assert.are.same({}, loader.stopped)
            assert.are.equal(1, restart_count)
        end)
    end)

    describe("widget", function()
        local function scenario()
            return fakeLoader{
                enabled = {
                    plugin("archiveviewer", nil, { fullname = "Archive viewer" }),
                    plugin("notesx", "/data/plugins/notesx.koplugin", { fullname = "Notes extra", version = "1.2" }),
                },
                disabled = { plugin("texteditor", nil, { fullname = "Text editor" }) },
                discovered = { { name = "broken", path = "/data/plugins/broken.koplugin" } },
                errors = { broken = "main.lua:3: boom" },
            }
        end

        it("builds the list with a row per plugin and the Extensions title", function()
            local widget = Extensions:new{ loader = scenario(), profile = Profile, safe_mode = false }
            assert.is_true(hasText(widget, "Extensions"))
            assert.is_true(hasText(widget, "Archive viewer"))
            assert.is_true(hasText(widget, "Notes extra"))
            assert.is_true(hasText(widget, "Text editor"))
            assert.is_true(hasTextPart(widget, "On. Bundled, not yet device tested"))
            assert.is_true(hasTextPart(widget, "On, Installed by you, version 1.2"))
            assert.is_true(hasText(widget, "Failed to load, Installed by you"))
            assert.is_false(hasTextPart(widget, "Safe mode is on"))
            paint(widget)
        end)

        it("shows the safe mode banner when safe mode is on", function()
            local widget = Extensions:new{ loader = scenario(), profile = Profile, safe_mode = true }
            assert.is_true(hasText(widget, "Safe mode is on. Third-party extensions are not loaded."))
            paint(widget)
        end)

        it("pages a long list and shows the pager", function()
            local many = {}
            for i = 1, 60 do
                table.insert(many, plugin("extra" .. i, "/data/plugins/extra" .. i .. ".koplugin", { fullname = "Extra " .. i }))
            end
            local widget = Extensions:new{ loader = fakeLoader{ enabled = many }, profile = Profile, safe_mode = false }
            assert.is_true(#widget.layout > 0)
            assert.is_true(hasTextPart(widget, "Page 1 of "))
            widget:turnPage(1)
            assert.is_true(hasTextPart(widget, "Page 2 of "))
            paint(widget)
        end)

        it("shows the detail view with the load error for a failed plugin", function()
            local widget = Extensions:new{ loader = scenario(), profile = Profile, safe_mode = false }
            local row = byName(widget.rows).broken
            widget:showDetail(row)
            assert.is_true(hasText(widget, "broken"))
            assert.is_true(hasText(widget, "Load error"))
            assert.is_true(hasText(widget, "main.lua:3: boom"))
            assert.is_true(hasText(widget, "Turn on"))
            paint(widget)
        end)

        it("turns a bundled plugin on from its detail view and asks for a restart", function()
            local loader = scenario()
            local widget = Extensions:new{ loader = loader, profile = Profile, safe_mode = false }
            local row = byName(widget.rows).texteditor
            widget:showDetail(row)
            assert.is_true(hasText(widget, "Turn on"))

            widget:toggleDetail()
            assert.is_nil(G_reader_settings:readSetting("plugins_disabled").texteditor)
            assert.are.equal(1, restart_count)
            assert.is_true(hasText(widget, "Turn off"))
            assert.is_true(hasTextPart(widget, "Restart Kaia to apply the change."))
            paint(widget)

            widget:showList()
            assert.is_nil(widget.detail)
        end)

        it("turns a running bundled plugin off and stops it", function()
            local loader = scenario()
            local widget = Extensions:new{ loader = loader, profile = Profile, safe_mode = false }
            widget:showDetail(byName(widget.rows).archiveviewer)
            widget:toggleDetail()
            assert.is_true(G_reader_settings:readSetting("plugins_disabled").archiveviewer)
            assert.are.same({ "archiveviewer" }, loader.stopped)
        end)

        it("does nothing from the detail view of a plugin that cannot be toggled", function()
            local loader = scenario()
            local widget = Extensions:new{ loader = loader, profile = Profile, safe_mode = false }
            widget:showDetail(byName(widget.rows).broken)
            widget:toggleDetail()
            assert.are.equal(0, restart_count)
            assert.is_nil(G_reader_settings:readSetting("plugins_disabled"))
        end)

        it("shows the safe mode note in the detail view of a skipped plugin", function()
            local loader = fakeLoader{
                disabled = { plugin("notesx", "/data/plugins/notesx.koplugin", { fullname = "Notes extra" }) },
            }
            local widget = Extensions:new{ loader = loader, profile = Profile, safe_mode = true }
            widget:showDetail(byName(widget.rows).notesx)
            assert.is_true(hasText(widget, "Skipped in safe mode"))
            assert.is_true(hasTextPart(widget, "Safe mode skips third-party plugins."))
            paint(widget)
        end)

        it("shows a message when there are no plugins", function()
            local widget = Extensions:new{ loader = fakeLoader{}, profile = Profile, safe_mode = false }
            assert.is_true(hasText(widget, "No extensions found."))
            paint(widget)
        end)
    end)
end)
