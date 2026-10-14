--[[--
Kaia's guard for DocSettings in the Rust plugin host.

KOReader writes a book's sidecar next to the book (`book.sdr/`) when the
`document_metadata_folder` setting is "doc". This guard pins that setting to
the value in frontend/kaia/defaults.lua, which is "dir": per-book data goes to
Kaia's data folder. A plugin or a settings file cannot change it.

The guard also loads KOReader's docsettings.lua with an `os` table that removes
and renames files only inside Kaia's folders. KOReader's module can name files
next to the book, such as `book.epub.kpdfview.lua`, and a stock sidecar can name
a cache file anywhere. The guard refuses those removals. The module itself is
unchanged.

DocSettings still reads KOReader's sidecars next to the book. It imports them
when Kaia has no copy yet.

@module docsettings_guard
]]

-- luacheck: globals KAIA_PATHS

local Defaults = require("kaia/defaults")

local Guard = {}

local KEY = "document_metadata_folder"
local PINNED = Defaults[KEY]
assert(PINNED == "dir", "Kaia writes sidecars to its data folder, not " .. tostring(PINNED))

--- Pins the metadata location on a LuaSettings object and returns it.
-- Reads return the pinned value, whatever the settings file or a plugin stored.
-- Writes and deletes store the pinned value, and so does every flush.
function Guard.pin(settings)
    local read_setting = settings.readSetting
    local save_setting = settings.saveSetting
    local del_setting = settings.delSetting
    local flush = settings.flush

    settings.readSetting = function(self, key, default)
        if key == KEY then
            return PINNED
        end
        return read_setting(self, key, default)
    end
    settings.saveSetting = function(self, key, value)
        if key == KEY then
            value = PINNED
        end
        return save_setting(self, key, value)
    end
    settings.delSetting = function(self, key)
        if key == KEY then
            return save_setting(self, key, PINNED)
        end
        return del_setting(self, key)
    end
    settings.flush = function(self, ...)
        self.data[KEY] = PINNED
        return flush(self, ...)
    end
    save_setting(settings, KEY, PINNED)
    return settings
end

--- The folders that Kaia owns. DocSettings may remove or rename files only
-- inside these folders.
local function kaiaFolders()
    local DataStorage = require("datastorage")
    return {
        DataStorage:getDocSettingsDir(),
        DataStorage:getDocSettingsHashDir(),
        DataStorage:getHistoryDir(),
        DataStorage:getDataDir() .. "/cache",
    }
end

--- True when `path` is inside one of `folders`. A `.` or `..` part is never
-- allowed, so a path cannot climb out of its folder.
local function isKaiaPath(path, folders)
    if type(path) ~= "string" or ("/" .. path .. "/"):find("/%.%.?/") then
        return false
    end
    for _, folder in ipairs(folders) do
        if path:sub(1, #folder + 1) == folder .. "/" then
            return true
        end
    end
    return false
end

local function refuse(action, path)
    require("logger").warn("Kaia refused to " .. action .. " " .. tostring(path) .. ", which is not a Kaia folder")
    return nil, "not a Kaia folder: " .. tostring(path)
end

--- Loads KOReader's docsettings.lua with the guarded `os`. `G_reader_settings`
-- in that module is the pinned object, so a plugin that rebinds the global
-- cannot change where DocSettings writes. Every other global is the same as in
-- the host.
local function loadDocSettings(settings)
    local folders = kaiaFolders()
    local real_os = os
    local guarded_os = setmetatable({
        remove = function(path, ...)
            if not isKaiaPath(path, folders) then
                return refuse("remove", path)
            end
            return real_os.remove(path, ...)
        end,
        rename = function(from, to, ...)
            if not (isKaiaPath(from, folders) and isKaiaPath(to, folders)) then
                return refuse("rename", from)
            end
            return real_os.rename(from, to, ...)
        end,
    }, { __index = real_os })
    local env = setmetatable({ os = guarded_os, G_reader_settings = settings }, { __index = _G })
    local chunk = assert(loadfile(KAIA_PATHS.koreader .. "/frontend/docsettings.lua"))
    setfenv(chunk, env)
    return chunk()
end

--- Makes `require("docsettings")` load the guarded module, with `settings` as
-- its settings object. Call it after `Guard.pin(settings)` and before anything
-- requires docsettings.
function Guard.install(settings)
    assert(settings:readSetting(KEY) == PINNED, "pin the settings before installing the guard")
    assert(package.loaded.docsettings == nil, "docsettings was loaded before the guard")
    package.preload.docsettings = function()
        return loadDocSettings(settings)
    end
end

return Guard
