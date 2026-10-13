--[[--
Runtime compatibility tracing for Kaia (KR-012).

Privacy rule: this module never records book text, file names, setting
values or widget text. Every recorded value is either a fixed label or an
identifier that matches a strict pattern. It records:

- module names passed to require, first load only;
- widget class names: widget.name when it is a plain identifier, otherwise
  the source path of the widget's init method;
- event handler names such as "onCloseDocument", never event arguments;
- timer delays for scheduleIn, nextTick and tickAfterNext;
- setting key names for LuaSettings and DocSettings, never values;
- file open modes, the file extension, and whether the path is in the
  plugins directory, the data directory or elsewhere. Neither the file name
  nor any directory name is recorded;
- main menu registrations, by plugin.

Enable with KAIA_TRACE=/path/to/trace.jsonl. See docs/kaia/inventory/TRACING.md.

@module kaia.trace
]]

local Trace = {}

local FLUSH_EVERY = 50
-- Stack levels searched for a plugin frame, counted from the record call.
local MAX_STACK_DEPTH = 8

-- Captured before any wrapper exists, so the trace writer never traces itself.
local io_open = io.open
local orig_require = require

-- Module name -> function that wraps the module's methods.
local patchers = {}
-- Patch order matters: luasettings must be wrapped before docsettings.
local PATCH_ORDER = {
    "luasettings",
    "docsettings",
    "ui/uimanager",
    "apps/reader/modules/readermenu",
    "apps/filemanager/filemanagermenu",
}

-- nil while not installed.
local state

local function is_ident(s, max_len)
    return type(s) == "string" and #s <= (max_len or 64) and s:match("^[%w_%.%-]+$") ~= nil
end

local function clean_ident(s)
    if is_ident(s) then return s end
    return "other"
end

local function clean_module(name)
    if type(name) == "string" and #name <= 128 and name:match("^[%w_%./%-]+$") then
        return name
    end
    return "other"
end

local function clean_mode(mode)
    if mode == nil then return "r" end
    if type(mode) == "string" and #mode <= 4 and mode:match("^[rwa%+bt]+$") then
        return mode
    end
    return "other"
end

local function clean_delay(seconds)
    if type(seconds) == "number" then return seconds end
    return "other"
end

-- Returns the module path for a source string such as
-- "@/x/frontend/ui/widget/infomessage.lua", or the plugin file path.
local function module_of_source(source)
    if type(source) ~= "string" then return "other" end
    local m = source:match("frontend/(.+)%.lua$")
    if not m then
        m = source:match("plugins/([^/]+%.koplugin/.+)%.lua$")
    end
    return clean_module(m)
end

local function plugin_of_source(source)
    if type(source) ~= "string" then return nil end
    return source:match("plugins/([^/]+)%.koplugin/")
end

-- Walks the stack from the record call outwards and returns the first
-- plugin whose main or helper file is on the stack, or "core".
local function caller_plugin()
    for level = 2, MAX_STACK_DEPTH do
        local info = debug.getinfo(level, "S")
        if not info then break end
        local name = plugin_of_source(info.source)
        if name then return name end
    end
    return "core"
end

local function widget_name(widget)
    if type(widget) ~= "table" then return "other" end
    local name = widget.name
    if is_ident(name) then return name end
    local init = widget.init
    if type(init) == "function" then
        return module_of_source(debug.getinfo(init, "S").source)
    end
    return "other"
end

local function event_handler(event)
    if type(event) ~= "table" then return "other" end
    return clean_ident(event.handler)
end

local function under_dir(dir, path)
    if type(dir) ~= "string" or dir == "" then return false end
    if dir == "." then
        return path:sub(1, 1) ~= "/"
    end
    dir = dir:gsub("/+$", "")
    return path:sub(1, #dir + 1) == dir .. "/"
end

local function location_of(path)
    if path:find("^plugins/") or path:find("/plugins/", 1, true) then
        return "plugins_dir"
    end
    if under_dir(state.data_dir, path) then
        return "data_dir"
    end
    return "elsewhere"
end

-- Returns the extension only, never the file name.
local function ext_of(path)
    local ext = path:match("%.([^%./\\]+)$")
    if not ext then return "none" end
    if #ext <= 8 and ext:match("^%w+$") then return ext:lower() end
    return "other"
end

local function json_string(s)
    return '"' .. (s:gsub('[%c"\\]', function(c)
        return string.format("\\u%04x", c:byte())
    end)) .. '"'
end

-- Minimal encoder for the record shape: strings, numbers, booleans and
-- tables with string keys. Used when rapidjson is not available.
local function json_value(v)
    local t = type(v)
    if t == "string" then return json_string(v) end
    if t == "boolean" then return tostring(v) end
    if t == "number" then
        if v ~= v or v == math.huge or v == -math.huge then return "null" end
        return string.format("%.6f", v)
    end
    if t == "table" then
        local keys = {}
        for k in pairs(v) do keys[#keys + 1] = tostring(k) end
        table.sort(keys)
        local parts = {}
        for i, k in ipairs(keys) do
            parts[i] = json_string(k) .. ":" .. json_value(v[k])
        end
        return "{" .. table.concat(parts, ",") .. "}"
    end
    return "null"
end

local function encode(rec)
    if state.rapidjson_encode then
        local ok, line = pcall(state.rapidjson_encode, rec)
        if ok and type(line) == "string" then return line end
    end
    return json_value(rec)
end

local function flush()
    if not state or #state.buffer == 0 then return end
    local data = table.concat(state.buffer, "\n") .. "\n"
    state.buffer = {}
    local f = io_open(state.path, "a")
    if f then
        f:write(data)
        f:close()
    end
end

-- Builds the detail table inside pcall, so an odd object cannot break the app.
local function record(kind, detail_fn)
    if not state then return end
    local ok, detail = pcall(detail_fn)
    if not ok or type(detail) ~= "table" then return end
    local ok_t, t = pcall(state.clock)
    if not ok_t then return end
    local rec = { t = t, kind = kind, plugin = caller_plugin(), detail = detail }
    local ok_e, line = pcall(encode, rec)
    if not ok_e then return end
    local buf = state.buffer
    buf[#buf + 1] = line
    if #buf >= FLUSH_EVERY then flush() end
end

-- Replaces tbl[key] and keeps the original for uninstall.
-- The original may be nil (an inherited method), in which case uninstall
-- sets the field back to nil.
local function replace(tbl, key, fn)
    table.insert(state.originals, { tbl = tbl, key = key, value = rawget(tbl, key) })
    rawset(tbl, key, fn)
end

local function wrap_method(tbl, key, make)
    local orig = rawget(tbl, key)
    if type(orig) ~= "function" then return end
    replace(tbl, key, make(orig))
end

-- Calls fn with timer tracing suppressed. nextTick and tickAfterNext call
-- scheduleIn internally, and only the outer call should be recorded.
local function quietly(fn, ...)
    local s = state
    if not s then return fn(...) end
    s.suppress = s.suppress + 1
    local res = table.pack(pcall(fn, ...))
    s.suppress = s.suppress - 1
    if not res[1] then error(res[2], 0) end
    return table.unpack(res, 2, res.n)
end

local function maybe_patch(name, mod)
    local patch = patchers[name]
    if not patch or state.patched[name] then return end
    mod = mod or package.loaded[name]
    if type(mod) ~= "table" then return end
    state.patched[name] = true
    pcall(patch, mod)
end

local function traced_require(name, ...)
    local existed = type(name) == "string" and package.loaded[name] ~= nil
    local results = table.pack(orig_require(name, ...))
    if state and type(name) == "string" then
        if not existed and not state.seen[name] and package.loaded[name] ~= nil then
            state.seen[name] = true
            record("require", function() return { module = clean_module(name) } end)
        end
        maybe_patch(name, results[1])
    end
    return table.unpack(results, 1, results.n)
end

local function traced_open(path, mode, ...)
    if state and type(path) == "string" then
        record("file_open", function()
            return {
                mode = clean_mode(mode),
                ext = ext_of(path),
                location = location_of(path),
            }
        end)
    end
    return io_open(path, mode, ...)
end

patchers["luasettings"] = function(LuaSettings)
    state.lua_settings_save = rawget(LuaSettings, "saveSetting")
    wrap_method(LuaSettings, "saveSetting", function(orig)
        return function(self, key, value)
            record("setting", function()
                return { store = "settings", key = clean_ident(key) }
            end)
            return orig(self, key, value)
        end
    end)
end

patchers["docsettings"] = function(DocSettings)
    if rawget(DocSettings, "saveSetting") ~= nil then return end
    -- DocSettings instances reach LuaSettings.saveSetting through
    -- inheritance. Give DocSettings its own wrapper, so each save is
    -- recorded once, under the store name "docsettings".
    local LuaSettings = package.loaded["luasettings"]
    local orig = state.lua_settings_save or (LuaSettings and rawget(LuaSettings, "saveSetting"))
    if type(orig) ~= "function" then return end
    replace(DocSettings, "saveSetting", function(self, key, value)
        record("setting", function()
            return { store = "docsettings", key = clean_ident(key) }
        end)
        return orig(self, key, value)
    end)
end

patchers["ui/uimanager"] = function(UIManager)
    wrap_method(UIManager, "show", function(orig)
        return function(self, widget, ...)
            record("widget_show", function()
                return { widget = widget_name(widget) }
            end)
            return orig(self, widget, ...)
        end
    end)

    wrap_method(UIManager, "close", function(orig)
        return function(self, widget, ...)
            record("widget_close", function()
                return { widget = widget_name(widget) }
            end)
            return orig(self, widget, ...)
        end
    end)

    wrap_method(UIManager, "scheduleIn", function(orig)
        return function(self, seconds, action, ...)
            if state and state.suppress == 0 then
                record("timer", function()
                    return { api = "scheduleIn", delay = clean_delay(seconds) }
                end)
            end
            return orig(self, seconds, action, ...)
        end
    end)

    wrap_method(UIManager, "nextTick", function(orig)
        return function(self, action, ...)
            record("timer", function() return { api = "nextTick", delay = 0 } end)
            return quietly(orig, self, action, ...)
        end
    end)

    -- The real delay is internal to UIManager, so this records 0.
    wrap_method(UIManager, "tickAfterNext", function(orig)
        return function(self, action, ...)
            record("timer", function() return { api = "tickAfterNext", delay = 0 } end)
            return quietly(orig, self, action, ...)
        end
    end)

    wrap_method(UIManager, "sendEvent", function(orig)
        return function(self, event)
            record("event", function()
                return { api = "sendEvent", handler = event_handler(event) }
            end)
            return orig(self, event)
        end
    end)

    wrap_method(UIManager, "broadcastEvent", function(orig)
        return function(self, event)
            record("event", function()
                return { api = "broadcastEvent", handler = event_handler(event) }
            end)
            return orig(self, event)
        end
    end)
end

local function patch_menu(menu_name)
    return function(Menu)
        wrap_method(Menu, "registerToMainMenu", function(orig)
            return function(self, widget)
                record("menu_register", function() return { menu = menu_name } end)
                return orig(self, widget)
            end
        end)
    end
end

patchers["apps/reader/modules/readermenu"] = patch_menu("reader")
patchers["apps/filemanager/filemanagermenu"] = patch_menu("filemanager")

local function default_clock(ffiutil)
    return function()
        local secs, usecs = ffiutil.gettime()
        return secs + usecs / 1000000
    end
end

--- Starts tracing. Idempotent. Truncates opts.path.
-- @tparam table opts `path` (string, required) and `clock` (optional function
-- returning seconds as a float; tests use this).
function Trace:install(opts)
    if state then return self end
    opts = opts or {}
    assert(type(opts.path) == "string" and opts.path ~= "", "kaia trace: opts.path is required")

    -- Load helpers before any wrapper exists, so loading them is not traced.
    local ok_rj, rapidjson = pcall(require, "rapidjson")
    local ok_util, ffiutil = pcall(require, "ffi/util")
    local ok_ds, DataStorage = pcall(require, "datastorage")

    local probe = io_open(opts.path, "w")
    assert(probe, "kaia trace: cannot open " .. opts.path .. " for writing")
    probe:close()

    local clock = opts.clock
    if not clock and ok_util and type(ffiutil.gettime) == "function" then
        clock = default_clock(ffiutil)
    end

    state = {
        path = opts.path,
        clock = clock or os.time,
        buffer = {},
        originals = {},
        seen = {},
        patched = {},
        suppress = 0,
        data_dir = ok_ds and DataStorage:getDataDir() or nil,
        rapidjson_encode = (ok_rj and type(rapidjson) == "table") and rapidjson.encode or nil,
    }

    replace(_G, "require", traced_require)
    replace(io, "open", traced_open)

    -- KOReader ends with os.exit, so write the partial last batch first.
    local orig_exit = os.exit
    replace(os, "exit", function(...)
        pcall(flush)
        return orig_exit(...)
    end)

    -- Patch modules that are already loaded. The rest are patched when
    -- traced_require sees them load.
    for _, name in ipairs(PATCH_ORDER) do
        if package.loaded[name] ~= nil then
            maybe_patch(name, package.loaded[name])
        end
    end

    return self
end

--- Writes buffered records to the trace file.
function Trace:flush()
    flush()
    return self
end

--- Flushes, then restores every original function. Safe to call when not installed.
function Trace:uninstall()
    if not state then return self end
    flush()
    for i = #state.originals, 1, -1 do
        local o = state.originals[i]
        rawset(o.tbl, o.key, o.value)
    end
    state = nil
    return self
end

return Trace
