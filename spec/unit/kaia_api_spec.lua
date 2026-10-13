describe("kaia extension API v1", function()
    local KaiaAPI, Dispatcher, UIManager
    local shown, real_show

    setup(function()
        require("commonrequire")
        disable_plugins()
        require("document/canvascontext"):init(require("device"))
        KaiaAPI = require("kaia/api/v1")
        Dispatcher = require("dispatcher")
        UIManager = require("ui/uimanager")
        real_show = UIManager.show
    end)

    before_each(function()
        shown = {}
        -- Record the widgets instead of showing them.
        UIManager.show = function(_self, widget)
            table.insert(shown, widget)
        end
    end)

    after_each(function()
        UIManager.show = real_show
    end)

    -- A plugin instance with a main menu that records the widgets it registers with.
    local function fakePlugin(name)
        local menu = { registered = {} }
        function menu:registerToMainMenu(widget)
            table.insert(self.registered, widget)
        end
        return {
            name = name,
            path = "plugins/" .. name .. ".koplugin",
            ui = { menu = menu },
        }
    end

    -- Menu items as KOReader would build them: every registered widget adds its items.
    local function menuItems(plugin)
        local items = {}
        for _i, widget in ipairs(plugin.ui.menu.registered) do
            widget:addToMainMenu(items)
        end
        return items
    end

    -- Returns the error message from a function that must raise one.
    local function errorOf(fn)
        local ok, err = pcall(fn)
        assert.is_false(ok)
        return tostring(err)
    end

    describe("versions", function()
        it("is version 1", function()
            assert.are.equal(1, KaiaAPI.version)
        end)

        it("returns the API for major version 1", function()
            assert.are.equal(KaiaAPI, KaiaAPI.require(1))
        end)

        it("returns nil and a reason for an unsupported major version", function()
            local api, reason = KaiaAPI.require(2)
            assert.is_nil(api)
            assert.truthy(reason:find("2", 1, true))
            assert.truthy(reason:find("version 1", 1, true))
        end)

        it("returns nil and a reason when the major version is not a number", function()
            local api, reason = KaiaAPI.require("1")
            assert.is_nil(api)
            assert.truthy(reason:find("number", 1, true))
        end)
    end)

    describe("argument validation", function()
        local plugin

        before_each(function()
            plugin = fakePlugin("valid_plugin")
        end)

        it("names the missing plugin", function()
            local msg = errorOf(function()
                KaiaAPI.addCommand{ id = "x", title = "X", run = function() end }
            end)
            assert.truthy(msg:find("'plugin'", 1, true))
        end)

        it("requires a plugin with a name", function()
            local msg = errorOf(function()
                KaiaAPI.addCommand{ plugin = {}, id = "x", title = "X", run = function() end }
            end)
            assert.truthy(msg:find("'plugin'", 1, true))
        end)

        it("names the bad id", function()
            local msg = errorOf(function()
                KaiaAPI.addCommand{ plugin = plugin, id = "1abc", title = "X", run = function() end }
            end)
            assert.truthy(msg:find("'id'", 1, true))
        end)

        it("names an empty title", function()
            local msg = errorOf(function()
                KaiaAPI.addCommand{ plugin = plugin, id = "x", title = "", run = function() end }
            end)
            assert.truthy(msg:find("'title'", 1, true))
        end)

        it("names a run that is not a function", function()
            local msg = errorOf(function()
                KaiaAPI.addCommand{ plugin = plugin, id = "x", title = "X", run = "nope" }
            end)
            assert.truthy(msg:find("'run'", 1, true))
        end)

        it("names an event_name that is not an identifier", function()
            local msg = errorOf(function()
                KaiaAPI.addCommand{ plugin = plugin, id = "x", title = "X", run = function() end, event_name = "bad-name" }
            end)
            assert.truthy(msg:find("'event_name'", 1, true))
        end)

        it("rejects a second command with the same id in one plugin", function()
            KaiaAPI.addCommand{ plugin = plugin, id = "dup_check", title = "X", run = function() end }
            local msg = errorOf(function()
                KaiaAPI.addCommand{ plugin = plugin, id = "dup_check", title = "Y", run = function() end }
            end)
            assert.truthy(msg:find("'id'", 1, true))
            assert.truthy(msg:find("already registered", 1, true))
        end)

        it("requires exactly one of run and sub_items for a menu item", function()
            local both = errorOf(function()
                KaiaAPI.addMenuItem{ plugin = plugin, id = "m", title = "M", run = function() end,
                    sub_items = { { title = "a", run = function() end } } }
            end)
            assert.truthy(both:find("'run' or 'sub_items'", 1, true))
            local neither = errorOf(function()
                KaiaAPI.addMenuItem{ plugin = plugin, id = "m2", title = "M" }
            end)
            assert.truthy(neither:find("'run' or 'sub_items'", 1, true))
        end)

        it("names an unknown section", function()
            local msg = errorOf(function()
                KaiaAPI.addMenuItem{ plugin = plugin, id = "m", title = "M", section = "bogus", run = function() end }
            end)
            assert.truthy(msg:find("'section'", 1, true))
        end)

        it("names an empty sub_items list", function()
            local msg = errorOf(function()
                KaiaAPI.addMenuItem{ plugin = plugin, id = "m", title = "M", sub_items = {} }
            end)
            assert.truthy(msg:find("'sub_items'", 1, true))
        end)

        it("names a sub item without a run function", function()
            local msg = errorOf(function()
                KaiaAPI.addMenuItem{ plugin = plugin, id = "m", title = "M",
                    sub_items = { { title = "a" } } }
            end)
            assert.truthy(msg:find("sub_items[1]", 1, true))
        end)

        it("names an unknown setting kind", function()
            local msg = errorOf(function()
                KaiaAPI.addSetting{ plugin = plugin, id = "s", title = "S", kind = "slider", default = 1 }
            end)
            assert.truthy(msg:find("'kind'", 1, true))
        end)

        it("requires a default for a setting", function()
            local msg = errorOf(function()
                KaiaAPI.addSetting{ plugin = plugin, id = "s", title = "S", kind = "toggle" }
            end)
            assert.truthy(msg:find("'default'", 1, true))
        end)

        it("checks the default type of a toggle", function()
            local msg = errorOf(function()
                KaiaAPI.addSetting{ plugin = plugin, id = "s", title = "S", kind = "toggle", default = "yes" }
            end)
            assert.truthy(msg:find("'default'", 1, true))
        end)

        it("requires choices for a choice setting", function()
            local msg = errorOf(function()
                KaiaAPI.addSetting{ plugin = plugin, id = "s", title = "S", kind = "choice", default = "a" }
            end)
            assert.truthy(msg:find("'choices'", 1, true))
        end)

        it("requires the default to be one of the choices", function()
            local msg = errorOf(function()
                KaiaAPI.addSetting{ plugin = plugin, id = "s", title = "S", kind = "choice",
                    choices = { "a", "b" }, default = "c" }
            end)
            assert.truthy(msg:find("'default'", 1, true))
        end)

        it("requires bounds for a number setting", function()
            local msg = errorOf(function()
                KaiaAPI.addSetting{ plugin = plugin, id = "s", title = "S", kind = "number", default = 2, max = 5 }
            end)
            assert.truthy(msg:find("'min'", 1, true))
        end)

        it("requires the default of a number setting to be inside the bounds", function()
            local msg = errorOf(function()
                KaiaAPI.addSetting{ plugin = plugin, id = "s", title = "S", kind = "number", default = 9, min = 1, max = 5 }
            end)
            assert.truthy(msg:find("'default'", 1, true))
        end)

        it("names an unknown message kind", function()
            local msg = errorOf(function()
                KaiaAPI.showMessage{ text = "hi", kind = "error" }
            end)
            assert.truthy(msg:find("'kind'", 1, true))
        end)

        it("requires text for a message", function()
            local msg = errorOf(function()
                KaiaAPI.showMessage{ kind = "info" }
            end)
            assert.truthy(msg:find("'text'", 1, true))
        end)

        it("requires on_ok for a confirmation", function()
            local msg = errorOf(function()
                KaiaAPI.confirm{ text = "Sure?" }
            end)
            assert.truthy(msg:find("'on_ok'", 1, true))
        end)

        it("rejects a call that is not a table", function()
            local msg = errorOf(function()
                KaiaAPI.showMessage("hi")
            end)
            assert.truthy(msg:find("table", 1, true))
        end)
    end)

    describe("settings", function()
        local plugin

        before_each(function()
            plugin = fakePlugin("settingplug")
        end)

        after_each(function()
            G_reader_settings:delSetting("kaia_ext_settingplug_loud")
            G_reader_settings:delSetting("kaia_ext_settingplug_level")
            G_reader_settings:delSetting("kaia_ext_settingplug_size")
            G_reader_settings:delSetting("kaia_ext_settingplug_bad")
        end)

        it("stores a toggle under kaia_ext_<plugin>_<id>", function()
            local get, set = KaiaAPI.addSetting{
                plugin = plugin, id = "loud", title = "Loud", kind = "toggle", default = false,
            }
            set(true)
            assert.is_true(G_reader_settings:readSetting("kaia_ext_settingplug_loud"))
            assert.is_true(get())
        end)

        it("returns the default while nothing is stored", function()
            G_reader_settings:delSetting("kaia_ext_settingplug_loud")
            local get = KaiaAPI.addSetting{
                plugin = plugin, id = "loud", title = "Loud", kind = "toggle", default = true,
            }
            assert.is_true(get())
        end)

        it("returns the default when the stored value has the wrong type", function()
            G_reader_settings:saveSetting("kaia_ext_settingplug_bad", "yes")
            local get = KaiaAPI.addSetting{
                plugin = plugin, id = "bad", title = "Bad", kind = "toggle", default = false,
            }
            assert.is_false(get())
        end)

        it("stores and reads a choice", function()
            local get, set = KaiaAPI.addSetting{
                plugin = plugin, id = "level", title = "Level", kind = "choice",
                choices = { "low", { value = "high", text = "High" } }, default = "low",
            }
            assert.are.equal("low", get())
            set("high")
            assert.are.equal("high", G_reader_settings:readSetting("kaia_ext_settingplug_level"))
            assert.are.equal("high", get())
        end)

        it("rejects a choice value that is not offered", function()
            local _, set = KaiaAPI.addSetting{
                plugin = plugin, id = "level", title = "Level", kind = "choice",
                choices = { "low" }, default = "low",
            }
            local msg = errorOf(function() set("extreme") end)
            assert.truthy(msg:find("value", 1, true))
        end)

        it("keeps a number inside its bounds", function()
            local get, set = KaiaAPI.addSetting{
                plugin = plugin, id = "size", title = "Size", kind = "number", default = 3, min = 1, max = 10,
            }
            assert.are.equal(3, get())
            set(7)
            assert.are.equal(7, get())
            local msg = errorOf(function() set(11) end)
            assert.truthy(msg:find("value", 1, true))
        end)

        it("rejects a value of the wrong type", function()
            local _, set = KaiaAPI.addSetting{
                plugin = plugin, id = "loud", title = "Loud", kind = "toggle", default = false,
            }
            errorOf(function() set("on") end)
            assert.is_nil(G_reader_settings:readSetting("kaia_ext_settingplug_loud"))
        end)

        it("adds a toggle menu item that reflects and flips the value", function()
            local get = KaiaAPI.addSetting{
                plugin = plugin, id = "loud", title = "Loud", kind = "toggle", default = false,
            }
            local item = menuItems(plugin)["kaia_ext_settingplug_setting_loud"]
            assert.are.equal("Loud", item.text)
            assert.are.equal("setting", item.sorting_hint)
            assert.is_false(item.checked_func())
            item.callback()
            assert.is_true(get())
            assert.is_true(item.checked_func())
        end)

        it("adds a choice menu item with one radio entry per choice", function()
            KaiaAPI.addSetting{
                plugin = plugin, id = "level", title = "Level", kind = "choice",
                choices = { "low", { value = "high", text = "High" } }, default = "low", section = "more_tools",
            }
            local item = menuItems(plugin)["kaia_ext_settingplug_setting_level"]
            assert.are.equal("more_tools", item.sorting_hint)
            assert.are.equal(2, #item.sub_item_table)
            assert.are.equal("High", item.sub_item_table[2].text)
            assert.is_true(item.sub_item_table[1].checked_func())
            item.sub_item_table[2].callback()
            assert.is_true(item.sub_item_table[2].checked_func())
            assert.is_false(item.sub_item_table[1].checked_func())
        end)

        it("adds a number menu item whose text shows the value", function()
            KaiaAPI.addSetting{
                plugin = plugin, id = "size", title = "Size", kind = "number", default = 3, min = 1, max = 10,
            }
            local item = menuItems(plugin)["kaia_ext_settingplug_setting_size"]
            assert.are.equal("Size: 3", item.text_func())
        end)
    end)

    describe("commands", function()
        it("registers a Dispatcher action with the title", function()
            local plugin = fakePlugin("cmdplug")
            local handle = KaiaAPI.addCommand{
                plugin = plugin, id = "say_hello", title = "Say hello", run = function() end,
            }
            assert.are.equal("kaia_ext_cmdplug_say_hello", handle.action)
            assert.are.equal("KaiaApi_cmdplug_say_hello", handle.event)

            local found
            for _i, item in ipairs(Dispatcher.getDisplayList({ handle.action })) do
                found = item.text
            end
            assert.are.equal("Say hello", found)
        end)

        it("calls run with the plugin ui when the Dispatcher event arrives", function()
            local plugin = fakePlugin("runplug")
            local got
            local handle = KaiaAPI.addCommand{
                plugin = plugin, id = "go", title = "Go", run = function(ui) got = ui end,
            }
            -- Dispatcher sends the event to the plugin instance, which looks up "on<event>".
            assert.is_function(plugin["on" .. handle.event])
            plugin["on" .. handle.event](plugin)
            assert.are.equal(plugin.ui, got)
        end)

        it("accepts an explicit event name", function()
            local plugin = fakePlugin("evplug")
            local handle = KaiaAPI.addCommand{
                plugin = plugin, id = "go", title = "Go", run = function() end, event_name = "KaiaEvGo",
            }
            assert.are.equal("KaiaEvGo", handle.event)
            assert.is_function(plugin.onKaiaEvGo)
        end)
    end)

    describe("menu items", function()
        it("registers one menu widget per plugin, however many items it adds", function()
            local plugin = fakePlugin("menuplug")
            KaiaAPI.addMenuItem{ plugin = plugin, id = "one", title = "One", run = function() end }
            KaiaAPI.addMenuItem{ plugin = plugin, id = "two", title = "Two", run = function() end }
            assert.are.equal(1, #plugin.ui.menu.registered)
        end)

        it("builds a KOReader menu item with the section hint and a callback", function()
            local plugin = fakePlugin("menuplug")
            local calls = {}
            local handle = KaiaAPI.addMenuItem{
                plugin = plugin, id = "greet", title = "Greet", section = "tools",
                run = function(ui) table.insert(calls, ui) end,
                enabled_func = function() return false end,
            }
            local item = menuItems(plugin)[handle.key]
            assert.are.equal("kaia_ext_menuplug_greet", handle.key)
            assert.are.equal("Greet", item.text)
            assert.are.equal("tools", item.sorting_hint)
            assert.is_false(item.enabled_func())
            assert.is_nil(item.sub_item_table)
            item.callback()
            assert.are.same({ plugin.ui }, calls)
        end)

        it("maps the settings section to the Settings tab key", function()
            local plugin = fakePlugin("menuplug")
            local handle = KaiaAPI.addMenuItem{
                plugin = plugin, id = "pref", title = "Pref", section = "settings", run = function() end,
            }
            assert.are.equal("setting", menuItems(plugin)[handle.key].sorting_hint)
        end)

        it("defaults to the more_tools section", function()
            local plugin = fakePlugin("menuplug")
            local handle = KaiaAPI.addMenuItem{ plugin = plugin, id = "misc", title = "Misc", run = function() end }
            assert.are.equal("more_tools", menuItems(plugin)[handle.key].sorting_hint)
        end)

        it("builds sub items with their own callbacks", function()
            local plugin = fakePlugin("menuplug")
            local ran = {}
            local handle = KaiaAPI.addMenuItem{
                plugin = plugin, id = "group", title = "Group",
                sub_items = {
                    { title = "First", run = function() table.insert(ran, "first") end },
                    { title = "Second", run = function() table.insert(ran, "second") end },
                },
            }
            local item = menuItems(plugin)[handle.key]
            assert.is_nil(item.callback)
            assert.are.equal(2, #item.sub_item_table)
            assert.are.equal("Second", item.sub_item_table[2].text)
            item.sub_item_table[2].callback()
            assert.are.same({ "second" }, ran)
        end)

        it("creates fresh item tables on each rebuild of the menu", function()
            local plugin = fakePlugin("menuplug")
            local handle = KaiaAPI.addMenuItem{ plugin = plugin, id = "again", title = "Again", run = function() end }
            local first = menuItems(plugin)[handle.key]
            local second = menuItems(plugin)[handle.key]
            assert.are_not.equal(first, second)
            assert.are.equal(first.text, second.text)
        end)
    end)

    describe("messages and confirmations", function()
        it("shows an info message with the info icon", function()
            local widget = KaiaAPI.showMessage{ text = "Saved." }
            assert.are.equal(widget, shown[1])
            assert.are.equal("Saved.", widget.text)
            assert.are.equal("notice-info", widget.icon)
        end)

        it("shows a warning with the warning icon", function()
            local widget = KaiaAPI.showMessage{ text = "Careful.", kind = "warning" }
            assert.are.equal("notice-warning", widget.icon)
        end)

        it("runs on_ok only from the OK callback", function()
            local ok_count = 0
            local widget = KaiaAPI.confirm{
                text = "Continue?", ok_text = "Continue", on_ok = function() ok_count = ok_count + 1 end,
            }
            assert.are.equal(widget, shown[1])
            assert.are.equal(0, ok_count)
            widget.ok_callback()
            assert.are.equal(1, ok_count)
            assert.are.equal("Continue", widget.ok_text)
        end)
    end)
end)
