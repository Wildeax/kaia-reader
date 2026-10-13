describe("kaia trace", function()
    local Trace
    local rapidjson
    -- Captured before any install, so reading the trace file is not traced.
    local real_open
    local SECRET_VALUE = "hunter2 private setting value"
    local SECRET_TEXT = "secret widget text"
    local SECRET_FILE = "secret-book-title.epub"
    local out_path

    local function make_clock()
        local t = 100
        return function()
            t = t + 0.5
            return t
        end
    end

    local function read(path)
        local f = assert(real_open(path, "r"))
        local s = f:read("*a")
        f:close()
        return s
    end

    local function decode_all(text)
        local recs = {}
        for line in text:gmatch("[^\n]+") do
            recs[#recs + 1] = rapidjson.decode(line)
        end
        return recs
    end

    local function find(recs, kind, field, value)
        for _, r in ipairs(recs) do
            if r.kind == kind and r.detail[field] == value then
                return r
            end
        end
    end

    local function count(recs, kind, field, value)
        local n = 0
        for _, r in ipairs(recs) do
            if r.kind == kind and r.detail[field] == value then
                n = n + 1
            end
        end
        return n
    end

    setup(function()
        require("commonrequire")
        Trace = require("kaia/trace")
        rapidjson = require("rapidjson")
        real_open = io.open
        out_path = require("datastorage"):getDataDir() .. "/kaia_trace_spec.jsonl"
    end)

    teardown(function()
        Trace:uninstall()
        os.remove(out_path)
    end)

    it("records module loads, widgets, timers, events, settings and file opens", function()
        local UIManager = require("ui/uimanager")
        local Widget = require("ui/widget/widget")
        local LuaSettings = require("luasettings")
        local DocSettings = require("docsettings")
        local Event = require("ui/event")
        package.preload["kaia_trace_probe_module"] = function() return {} end

        Trace:install{ path = out_path, clock = make_clock() }
        require("kaia_trace_probe_module")

        local widget = Widget:new{ name = "kaia_trace_probe_widget", text = SECRET_TEXT }
        UIManager:show(widget)
        UIManager:close(widget)
        -- A name with spaces is not an identifier, so it must not be recorded.
        local titled = Widget:new{ name = SECRET_TEXT }
        UIManager:show(titled)
        UIManager:close(titled)

        local action = function() end
        UIManager:scheduleIn(5, action)
        UIManager:unschedule(action)

        LuaSettings:wrap({}):saveSetting("kaia_probe_key", SECRET_VALUE)
        DocSettings:extend{ data = {} }:saveSetting("kaia_doc_key", SECRET_VALUE)

        UIManager:sendEvent(Event:new("KaiaTraceProbe", SECRET_VALUE))

        local f = io.open("/elsewhere/" .. SECRET_FILE, "rb")
        if f then f:close() end
        Trace:uninstall()

        local content = read(out_path)
        local recs = decode_all(content)

        local req = find(recs, "require", "module", "kaia_trace_probe_module")
        assert.is_truthy(req)
        assert.are.equal("core", req.plugin)
        assert.is_number(req.t)

        assert.is_truthy(find(recs, "widget_show", "widget", "kaia_trace_probe_widget"))
        assert.is_truthy(find(recs, "widget_close", "widget", "kaia_trace_probe_widget"))
        assert.is_truthy(find(recs, "widget_show", "widget", "other"))

        local timer = find(recs, "timer", "api", "scheduleIn")
        assert.is_truthy(timer)
        assert.are.equal(5, timer.detail.delay)

        assert.is_truthy(find(recs, "setting", "key", "kaia_probe_key"))
        local doc_set = find(recs, "setting", "key", "kaia_doc_key")
        assert.is_truthy(doc_set)
        assert.are.equal("docsettings", doc_set.detail.store)
        -- DocSettings saves go through one wrapper only, so each save is recorded once.
        assert.are.equal(1, count(recs, "setting", "key", "kaia_doc_key"))

        assert.is_truthy(find(recs, "event", "handler", "onKaiaTraceProbe"))

        local open = find(recs, "file_open", "ext", "epub")
        assert.is_truthy(open)
        assert.are.equal("rb", open.detail.mode)
        assert.are.equal("elsewhere", open.detail.location)

        -- Privacy: no setting value, widget text, event argument or file name.
        assert.is_nil(content:find(SECRET_VALUE, 1, true))
        assert.is_nil(content:find(SECRET_TEXT, 1, true))
        assert.is_nil(content:find(SECRET_FILE, 1, true))
        assert.is_nil(content:find("secret-book-title", 1, true))
        assert.is_nil(content:find("/elsewhere", 1, true))
    end)

    it("records a nextTick once, without a nested scheduleIn record", function()
        local UIManager = require("ui/uimanager")
        Trace:install{ path = out_path, clock = make_clock() }
        local action = function() end
        UIManager:nextTick(action)
        UIManager:unschedule(action)
        Trace:uninstall()

        local recs = decode_all(read(out_path))
        assert.are.equal(1, count(recs, "timer", "api", "nextTick"))
        assert.are.equal(0, count(recs, "timer", "api", "scheduleIn"))
    end)

    it("attributes records to the plugin on the call stack", function()
        package.preload["kaia_trace_probe_plugin"] = function() return {} end
        Trace:install{ path = out_path, clock = make_clock() }
        -- Not a tail call: a tail-called require would drop the plugin frame.
        local chunk = assert(load('local m = require("kaia_trace_probe_plugin") return m',
            "@/tmp/plugins/probe.koplugin/main.lua"))
        chunk()
        Trace:uninstall()

        local rec = find(decode_all(read(out_path)), "require", "module", "kaia_trace_probe_plugin")
        assert.is_truthy(rec)
        assert.are.equal("probe", rec.plugin)
    end)

    it("records main menu registration by plugin", function()
        local FileManagerMenu = require("apps/filemanager/filemanagermenu")
        Trace:install{ path = out_path, clock = make_clock() }
        local menu = setmetatable({ registered_widgets = {} }, { __index = FileManagerMenu })
        local register = assert(load("local menu, item = ...; menu:registerToMainMenu(item)",
            "@/tmp/plugins/menuprobe.koplugin/main.lua"))
        register(menu, {})
        Trace:uninstall()

        local rec = find(decode_all(read(out_path)), "menu_register", "menu", "filemanager")
        assert.is_truthy(rec)
        assert.are.equal("menuprobe", rec.plugin)
        assert.are.equal(1, #menu.registered_widgets)
    end)

    it("restores every original after uninstall, and install is idempotent", function()
        local UIManager = require("ui/uimanager")
        local LuaSettings = require("luasettings")
        local DocSettings = require("docsettings")
        local ReaderMenu = require("apps/reader/modules/readermenu")
        local FileManagerMenu = require("apps/filemanager/filemanagermenu")

        local originals = {
            require = require,
            open = io.open,
            exit = os.exit,
            show = UIManager.show,
            close = UIManager.close,
            scheduleIn = UIManager.scheduleIn,
            nextTick = UIManager.nextTick,
            tickAfterNext = UIManager.tickAfterNext,
            sendEvent = UIManager.sendEvent,
            broadcastEvent = UIManager.broadcastEvent,
            saveSetting = LuaSettings.saveSetting,
            docSaveSetting = rawget(DocSettings, "saveSetting"),
            reader_register = ReaderMenu.registerToMainMenu,
            fm_register = FileManagerMenu.registerToMainMenu,
        }

        Trace:install{ path = out_path, clock = make_clock() }
        Trace:install{ path = out_path, clock = make_clock() }
        assert.are_not.equal(originals.show, UIManager.show)
        assert.are_not.equal(originals.require, require)
        Trace:uninstall()

        assert.are.equal(originals.require, require)
        assert.are.equal(originals.open, io.open)
        assert.are.equal(originals.exit, os.exit)
        assert.are.equal(originals.show, UIManager.show)
        assert.are.equal(originals.close, UIManager.close)
        assert.are.equal(originals.scheduleIn, UIManager.scheduleIn)
        assert.are.equal(originals.nextTick, UIManager.nextTick)
        assert.are.equal(originals.tickAfterNext, UIManager.tickAfterNext)
        assert.are.equal(originals.sendEvent, UIManager.sendEvent)
        assert.are.equal(originals.broadcastEvent, UIManager.broadcastEvent)
        assert.are.equal(originals.saveSetting, LuaSettings.saveSetting)
        assert.are.equal(originals.docSaveSetting, rawget(DocSettings, "saveSetting"))
        assert.are.equal(originals.reader_register, ReaderMenu.registerToMainMenu)
        assert.are.equal(originals.fm_register, FileManagerMenu.registerToMainMenu)
    end)

    it("buffers records and writes them every 50", function()
        for i = 1, 50 do
            package.preload["kaia_trace_buffer_" .. i] = function() return {} end
        end
        Trace:install{ path = out_path, clock = make_clock() }
        for i = 1, 49 do
            require("kaia_trace_buffer_" .. i)
        end
        assert.are.equal("", read(out_path))
        require("kaia_trace_buffer_50")
        assert.are.equal(50, #decode_all(read(out_path)))
        Trace:uninstall()
    end)

    it("writes parsable lines with the fallback encoder when rapidjson is missing", function()
        package.preload["kaia_trace_probe_fallback"] = function() return {} end
        local saved = package.loaded.rapidjson
        package.loaded.rapidjson = false
        Trace:install{ path = out_path, clock = make_clock() }
        require("kaia_trace_probe_fallback")
        Trace:uninstall()
        package.loaded.rapidjson = saved

        local content = read(out_path)
        local rec = find(decode_all(content), "require", "module", "kaia_trace_probe_fallback")
        assert.is_truthy(rec)
        assert.are.equal("core", rec.plugin)
    end)
end)
