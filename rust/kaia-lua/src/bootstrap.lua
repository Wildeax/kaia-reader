-- Startup for the Kaia Lua host. Runs once per LuaHost, before any other Lua
-- code. It mirrors setupkoenv.lua and the non-UI part of reader.lua (the
-- locale, the defaults and the reader settings). It never loads device,
-- document or UI modules.
--
-- Arguments: shim directory, KOReader tree (the installed koreader/ layout),
-- Kaia data directory.
-- luacheck: globals KAIA_PATHS G_defaults G_reader_settings dofile

local shim_dir, koreader_dir, data_dir = ...

-- Absolute search paths: the working directory does not matter. The shims come
-- first. The order follows setupkoenv.lua.
package.path = shim_dir .. "/?.lua;"
    .. koreader_dir .. "/common/?.lua;"
    .. koreader_dir .. "/frontend/?.lua;"
    .. koreader_dir .. "/plugins/exporter.koplugin/?.lua;"
    .. koreader_dir .. "/?.lua;"
    .. package.path
package.cpath = koreader_dir .. "/common/?.so;"
    .. koreader_dir .. "/?.so;"
    .. package.cpath

KAIA_PATHS = {
    shim = shim_dir,
    koreader = koreader_dir,
    data = data_dir,
    libs = koreader_dir .. "/libs",
}

-- The same numeric locale as reader.lua.
os.setlocale("C", "numeric")

-- setupkoenv.lua loads ffi/loadlib, which replaces ffi.load and ffi.loadlib.
require("ffi/loadlib")

-- KOReader's loadlib searches "libs/?" relative to the working directory. Point
-- that search path and ffi.load("libs/...") at the KOReader tree.
local libs_dir = KAIA_PATHS.libs

-- Replaces every "libs/?" entry of a search path with the absolute one.
local function absolute_libs(path)
    local out, start = {}, 1
    while true do
        local s, e = path:find("libs/?", start, true)
        if not s then
            break
        end
        out[#out + 1] = path:sub(start, s - 1) .. libs_dir .. "/?"
        start = e + 1
    end
    out[#out + 1] = path:sub(start)
    return table.concat(out)
end

local ffi = require("ffi")
local ffi_load = ffi.load
ffi.load = function(lib, global)
    if lib:sub(1, 5) == "libs/" then
        lib = KAIA_PATHS.koreader .. "/" .. lib
    end
    return ffi_load(lib, global)
end

local searchpath = package.searchpath
package.searchpath = function(name, path, ...) -- luacheck: ignore 122
    if type(path) == "string" and path:find("libs/?", 1, true) then
        path = absolute_libs(path)
    end
    return searchpath(name, path, ...)
end

-- KOReader's luadefaults reads "defaults.lua" through dofile, relative to the
-- working directory. Only while it opens, that name maps to the KOReader tree.
local real_dofile = dofile
dofile = function(name, ...)
    if name == "defaults.lua" then
        name = koreader_dir .. "/defaults.lua"
    end
    return real_dofile(name, ...)
end
local ok, defaults = pcall(function()
    return require("luadefaults"):open()
end)
dofile = real_dofile
if not ok then
    error(defaults, 0)
end
G_defaults = defaults

-- The same settings file as reader.lua (lines 37 to 43), in the Kaia data folder.
G_reader_settings = require("luasettings"):open(data_dir .. "/settings.reader.lua")

-- Sidecars go to Kaia's data folder, never next to a book. The guard pins the
-- metadata location and loads DocSettings so that it cannot remove or rename
-- files outside Kaia's folders. See docsettings_guard.lua.
local sidecar_guard = require("docsettings_guard")
sidecar_guard.pin(G_reader_settings)
sidecar_guard.install(G_reader_settings)

return true
