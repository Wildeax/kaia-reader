--[[--
One-time, read-only import of stock KOReader data into Kaia (KR-016).

Kaia has its own data directory, so on first run it starts with empty settings
and history. This module copies the parts of KOReader's data directory that
Kaia can use: settings, reading history, per-book sidecars and plugin data.
It never writes to KOReader's directory and never replaces a file that Kaia
already has.

Not copied: user patches and user plugins (they are code), caches that rebuild
(bookinfo cache), KOSync's pending upload queue, screenshots, OTA files and
books.

Usage:
    local ImportKO = require("kaia/importko")
    local source = ImportKO.findSource()
    local plan = ImportKO.plan(source, DataStorage:getDataDir())
    local report = ImportKO.run(source, DataStorage:getDataDir())

@module kaia.importko
]]

local ConfirmBox = require("ui/widget/confirmbox")
local DataStorage = require("datastorage")
local InfoMessage = require("ui/widget/infomessage")
local LuaSettings = require("luasettings")
local UIManager = require("ui/uimanager")
local ffiutil = require("ffi/util")
local lfs = require("libs/libkoreader-lfs")
local logger = require("logger")
local util = require("util")
local _ = require("gettext")
local N_ = require("gettext").ngettext
local T = ffiutil.template

local ImportKO = {}

-- Files larger than this are not copied. The limit keeps the copy within the
-- Kobo's free space and memory budget.
ImportKO.MAX_FILE_SIZE = 200 * 1024 * 1024

-- Name of KOReader's data directory, next to Kaia's.
local SOURCE_DIR_NAME = "koreader"
local SETTINGS_FILE = "settings.reader.lua"
local HISTORY_FILE = "history.lua"
local OFFER_FLAG = "kaia_import_offer_shown"
local IMPORTED_FLAG = "kaia_imported_from_koreader"
local TMP_SUFFIX = ".kaia-import-tmp"
local COPY_CHUNK = 1024 * 1024
local LIST_LIMIT = 8

-- Settings that tie KOReader's install to one device or profile.
local DROPPED_SETTINGS = {
    start_with = true,
    document_metadata_folder = true,
    device_id = true,
    quickstart_shown_version = true,
}
-- Settings whose path value is kept only when the path still exists.
local PATH_SETTINGS = {
    lastdir = true,
    home_dir = true,
}
-- Plugin data that is rebuilt or is not safe to replay in Kaia.
local SKIPPED_PLUGIN_FILES = {
    ["kosync_queue.lua"] = true,
}

local symattr = lfs.symlinkattributes or lfs.attributes

local function trimSlash(path)
    if path and #path > 1 then
        return (path:gsub("/+$", ""))
    end
    return path
end

local function dirname(path)
    local parent = path:match("^(.*)/[^/]*$")
    if parent == nil then
        return "."
    end
    if parent == "" then
        return "/"
    end
    return parent
end

local function isFile(path)
    return lfs.attributes(path, "mode") == "file"
end

local function samePath(a, b)
    return (ffiutil.realpath(a) or a) == (ffiutil.realpath(b) or b)
end

--- Returns the KOReader data directory for Kaia, or nil.
--
-- The default is the sibling `koreader` directory of Kaia's data directory. On
-- Kobo that is `.adds/koreader`. On desktop it is `koreader` next to `kaia`.
-- The directory must contain `settings.reader.lua`.
-- @tparam[opt] table opts `dest` is Kaia's data directory (default: DataStorage),
--   `source` is an explicit KOReader directory (used by tests)
-- @treturn string|nil
function ImportKO.findSource(opts)
    opts = opts or {}
    local dest = trimSlash(opts.dest or DataStorage:getDataDir())
    local candidate = trimSlash(opts.source or (dirname(dest) .. "/" .. SOURCE_DIR_NAME))
    if lfs.attributes(candidate, "mode") ~= "directory" then
        return nil
    end
    if not isFile(candidate .. "/" .. SETTINGS_FILE) then
        return nil
    end
    if samePath(candidate, dest) then
        return nil
    end
    return candidate
end

local function isDroppedSetting(key, value)
    if type(key) ~= "string" then
        return false
    end
    if DROPPED_SETTINGS[key] or key:find("^kaia_") or key:find("^ota_") then
        return true
    end
    if PATH_SETTINGS[key] then
        return type(value) ~= "string" or lfs.attributes(value, "mode") == nil
    end
    return false
end

--- Merges KOReader's settings into Kaia's.
--
-- KOReader's values fill in keys that Kaia does not have yet. Kaia's values
-- always win. Keys that tie the install to one device or profile are dropped
-- (see DROPPED_SETTINGS).
-- @tparam table ko_data KOReader's settings table
-- @tparam table kaia_data Kaia's settings table
-- @treturn table merged settings
-- @treturn table summary `added`, `kept` and `dropped` (sorted key names)
function ImportKO.mergeSettings(ko_data, kaia_data)
    ko_data = ko_data or {}
    kaia_data = kaia_data or {}
    local merged = {}
    local summary = { added = 0, kept = 0, dropped = {} }
    for key, value in pairs(ko_data) do
        if isDroppedSetting(key, value) then
            table.insert(summary.dropped, key)
        elseif kaia_data[key] == nil then
            merged[key] = value
            summary.added = summary.added + 1
        else
            summary.kept = summary.kept + 1
        end
    end
    for key, value in pairs(kaia_data) do
        merged[key] = value
    end
    table.sort(summary.dropped, function(a, b) return tostring(a) < tostring(b) end)
    return merged, summary
end

-- Calls fn(rel, path, size, mtime) for each regular file below dir.
-- Symbolic links are not followed.
local function eachFile(dir, prefix, fn)
    if lfs.attributes(dir, "mode") ~= "directory" then
        return
    end
    for name in lfs.dir(dir) do
        if name ~= "." and name ~= ".." then
            local path = dir .. "/" .. name
            local rel = prefix .. name
            local mode = symattr(path, "mode")
            if mode == "directory" then
                eachFile(path, rel .. "/", fn)
            elseif mode == "file" then
                fn(rel, path, lfs.attributes(path, "size") or 0, lfs.attributes(path, "modification"))
            end
        end
    end
end

-- Sorted names of the entries directly inside dir.
local function listNames(dir)
    local names = {}
    if lfs.attributes(dir, "mode") == "directory" then
        for name in lfs.dir(dir) do
            if name ~= "." and name ~= ".." then
                table.insert(names, name)
            end
        end
    end
    table.sort(names)
    return names
end

-- Sidecar files: `metadata.<ext>.lua` (and its .old backup), `custom_metadata.lua`
-- and a custom `cover.<ext>`. They must sit in a `<book>.sdr` folder.
local function isSidecar(rel)
    local name = rel:match("([^/]+)$")
    local parent = rel:match("([^/]+)/[^/]+$")
    if not parent or not parent:find("%.sdr$") then
        return false
    end
    return name == "custom_metadata.lua"
        or name:find("^metadata%..+%.lua$") ~= nil
        or name:find("^metadata%..+%.lua%.old$") ~= nil
        or name:find("^cover%.%w+$") ~= nil
end

local function isSkippedPluginFile(rel)
    local name = rel:match("([^/]+)$")
    return SKIPPED_PLUGIN_FILES[name] == true
        or name:find("^bookinfo_cache%.sqlite3") ~= nil
        or name:find("%.old$") ~= nil
        or name:find("%-journal$") ~= nil
        or name:find("%-wal$") ~= nil
        or name:find("%-shm$") ~= nil
end

-- Number of books in a history file. Zero when the file cannot be read.
local function countHistory(path)
    local ok, data = pcall(dofile, path)
    if ok and type(data) == "table" then
        return #data
    end
    return 0
end

local function readSettings(path, settings)
    if settings then
        return settings.data
    end
    return LuaSettings:open(path).data
end

--- Describes what a run would copy. Writes nothing.
-- @string source KOReader data directory (see findSource)
-- @string dest Kaia data directory
-- @tparam[opt] table opts `max_file_size` (bytes, default MAX_FILE_SIZE),
--   `settings` (Kaia's live settings object, default: read from dest)
-- @treturn table plan, with `items` (files to copy), `existing`, `too_large`,
--   `patches` and `code_plugins` (not copied), `settings` summary, and counts
function ImportKO.plan(source, dest, opts)
    opts = opts or {}
    source, dest = trimSlash(source), trimSlash(dest)
    local limit = opts.max_file_size or ImportKO.MAX_FILE_SIZE
    local p = {
        source = source,
        dest = dest,
        limit = limit,
        items = {},
        existing = {},
        too_large = {},
        history_books = 0,
        book_files = 0,
        plugin_files = 0,
        total_bytes = 0,
        patches = listNames(source .. "/patches"),
        code_plugins = listNames(source .. "/plugins"),
    }

    -- Returns true when the file is queued for copying.
    local function add(kind, rel, src, size, mtime)
        local dst = dest .. "/" .. rel
        if lfs.attributes(dst, "mode") then
            table.insert(p.existing, rel)
        elseif size > limit then
            table.insert(p.too_large, { rel = rel, size = size })
        else
            table.insert(p.items, { kind = kind, rel = rel, src = src, dst = dst, size = size, mtime = mtime })
            p.total_bytes = p.total_bytes + size
            if kind == "sidecar" then
                p.book_files = p.book_files + 1
            elseif kind == "plugin" or kind == "styletweak" then
                p.plugin_files = p.plugin_files + 1
            end
            return true
        end
        return false
    end

    local history_src = source .. "/" .. HISTORY_FILE
    if isFile(history_src) then
        if add("history", HISTORY_FILE, history_src,
               lfs.attributes(history_src, "size") or 0, lfs.attributes(history_src, "modification")) then
            p.history_books = countHistory(history_src)
        end
    end

    for _i, root in ipairs({ "docsettings", "hashdocsettings" }) do
        eachFile(source .. "/" .. root, root .. "/", function(rel, path, size, mtime)
            if isSidecar(rel) then
                add("sidecar", rel, path, size, mtime)
            end
        end)
    end

    eachFile(source .. "/settings", "settings/", function(rel, path, size, mtime)
        if not isSkippedPluginFile(rel) then
            add("plugin", rel, path, size, mtime)
        end
    end)

    eachFile(source .. "/styletweaks", "styletweaks/", function(rel, path, size, mtime)
        add("styletweak", rel, path, size, mtime)
    end)

    local ko_settings = readSettings(source .. "/" .. SETTINGS_FILE)
    local kaia_settings = readSettings(dest .. "/" .. SETTINGS_FILE, opts.settings)
    p.settings = select(2, ImportKO.mergeSettings(ko_settings, kaia_settings))

    table.sort(p.items, function(a, b) return a.rel < b.rel end)
    table.sort(p.existing)
    table.sort(p.too_large, function(a, b) return a.rel < b.rel end)
    return p
end

-- Copies src to dst through a temp file, then renames it into place. The copy
-- keeps the source's modification time. Returns true, or nil and an error.
local function copyAtomic(src, dst, size, mtime)
    util.makePath(dirname(dst))
    local fin, err = io.open(src, "rb")
    if not fin then
        return nil, err
    end
    local tmp = dst .. TMP_SUFFIX
    local fout
    fout, err = io.open(tmp, "wb")
    if not fout then
        fin:close()
        return nil, err
    end
    local written = 0
    local ok = true
    while true do
        local chunk = fin:read(COPY_CHUNK)
        if not chunk then
            break
        end
        local wrote, werr = fout:write(chunk)
        if not wrote then
            ok, err = nil, werr
            break
        end
        written = written + #chunk
    end
    fin:close()
    if ok and written ~= size then
        ok, err = nil, "file changed while copying"
    end
    if ok then
        ok, err = fout:flush()
        if ok then
            ffiutil.fsyncOpenedFile(fout)
        end
    end
    fout:close()
    if ok then
        ok, err = os.rename(tmp, dst)
    end
    if ok then
        if mtime then
            lfs.touch(dst, mtime, mtime)
        end
        ffiutil.fsyncDirectory(dst)
        return true
    end
    os.remove(tmp)
    return nil, err
end

-- Merges KOReader's settings into Kaia's settings and records the import time.
-- Kaia's values win. Returns the merge summary.
local function writeSettings(source, dest, opts)
    local settings = opts.settings or LuaSettings:open(dest .. "/" .. SETTINGS_FILE)
    local merged, summary = ImportKO.mergeSettings(
        readSettings(source .. "/" .. SETTINGS_FILE),
        settings.data)
    merged[IMPORTED_FLAG] = os.time()
    settings:reset(merged)
    settings:flush()
    return summary
end

--- Copies the data that plan lists. Never changes files in source.
--
-- Files that Kaia already has are kept and counted. Settings are merged and
-- the import time is recorded last, so a failed run does not look complete.
-- @string source KOReader data directory
-- @string dest Kaia data directory
-- @tparam[opt] table opts the same options as plan
-- @treturn table report, with `copied`, `bytes`, `history_books`, `existing`,
--   `too_large`, `failed`, `settings`, `not_copied` and `settings_written`
function ImportKO.run(source, dest, opts)
    opts = opts or {}
    local p = ImportKO.plan(source, dest, opts)
    local report = {
        source = p.source,
        dest = p.dest,
        copied = 0,
        bytes = 0,
        history_books = 0,
        existing = {},
        too_large = p.too_large,
        failed = {},
        not_copied = { patches = p.patches, plugins = p.code_plugins },
        settings_written = false,
    }
    for _i, rel in ipairs(p.existing) do
        table.insert(report.existing, rel)
    end
    for _i, item in ipairs(p.items) do
        if lfs.attributes(item.dst, "mode") then
            table.insert(report.existing, item.rel)
        else
            local ok, err = copyAtomic(item.src, item.dst, item.size, item.mtime)
            if ok then
                report.copied = report.copied + 1
                report.bytes = report.bytes + item.size
                if item.kind == "history" then
                    report.history_books = p.history_books
                end
            else
                logger.warn("kaia/importko: failed to copy", item.rel, err)
                table.insert(report.failed, { rel = item.rel, err = err })
            end
        end
    end
    local ok, summary = pcall(writeSettings, source, dest, opts)
    if ok then
        report.settings = summary
        report.settings_written = true
    else
        logger.warn("kaia/importko: failed to write settings", summary)
        report.settings = p.settings
        table.insert(report.failed, { rel = SETTINGS_FILE, err = summary })
    end
    return report
end

-- Lists up to LIST_LIMIT names, then "and N more".
local function listText(names)
    local shown = {}
    for i = 1, math.min(#names, LIST_LIMIT) do
        shown[i] = names[i]
    end
    local text = table.concat(shown, ", ")
    if #names > LIST_LIMIT then
        text = text .. ", " .. T(_("and %1 more"), #names - LIST_LIMIT)
    end
    return text
end

-- Joins parts with commas and a final "and".
local function joinAnd(parts)
    if #parts <= 1 then
        return parts[1] or ""
    end
    local head = table.concat(parts, ", ", 1, #parts - 1)
    return T(_("%1 and %2"), head, parts[#parts])
end

-- Names of the files that are listed but not copied (patches and user plugins).
local function notCopiedLines(not_copied, lines)
    if #not_copied.patches > 0 then
        table.insert(lines, T(_("Not copied, patches are code. Copy them by hand if you want them: %1"),
            listText(not_copied.patches)))
    end
    if #not_copied.plugins > 0 then
        table.insert(lines, T(_("Not copied, plugins are code. Copy them by hand if you want them: %1"),
            listText(not_copied.plugins)))
    end
end

-- "name (size)" for each file over the size limit.
local function tooLargeText(too_large)
    local names = {}
    for _i, item in ipairs(too_large) do
        table.insert(names, T(_("%1 (%2)"), item.rel, util.getFriendlySize(item.size)))
    end
    return listText(names)
end

--- Text for the confirmation dialog, built from a plan from ImportKO.plan.
-- @treturn string
function ImportKO.formatPlan(p)
    local added = p.settings.added
    local parts = { T(N_("%1 setting", "%1 settings", added), added) }
    if p.history_books > 0 then
        table.insert(parts, T(N_("reading history for %1 book", "reading history for %1 books", p.history_books),
            p.history_books))
    end
    if p.book_files > 0 then
        table.insert(parts, T(N_("%1 book data file", "%1 book data files", p.book_files), p.book_files))
    end
    if p.plugin_files > 0 then
        table.insert(parts, _("these plugin files"))
    end
    local lines = { T(_("Copy %1. KOReader's files are not changed."), joinAnd(parts)) }

    local plugin_names = {}
    for _i, item in ipairs(p.items) do
        if item.kind == "plugin" or item.kind == "styletweak" then
            table.insert(plugin_names, item.rel)
        end
    end
    if #plugin_names > 0 then
        table.insert(lines, T(_("Plugin files: %1"), listText(plugin_names)))
    end
    if #p.existing > 0 then
        local n = #p.existing
        table.insert(lines, T(N_("%1 file already in Kaia, not replaced.", "%1 files already in Kaia, not replaced.", n), n))
    end
    if #p.too_large > 0 then
        table.insert(lines, T(_("Skipped, larger than %1: %2"),
            util.getFriendlySize(p.limit), tooLargeText(p.too_large)))
    end
    notCopiedLines({ patches = p.patches, plugins = p.code_plugins }, lines)
    table.insert(lines, T(_("Total size: %1."), util.getFriendlySize(p.total_bytes) or "0 B"))
    return table.concat(lines, "\n")
end

--- Text for the result dialog, built from a report from ImportKO.run.
-- @treturn string
function ImportKO.formatReport(r)
    local lines = {}
    table.insert(lines, T(N_("Copied %1 file.", "Copied %1 files.", r.copied), r.copied))
    if r.settings then
        table.insert(lines, T(_("Settings: %1 added, %2 already in Kaia kept."),
            r.settings.added, r.settings.kept))
    end
    if r.history_books > 0 then
        table.insert(lines, T(N_("Reading history: %1 book.", "Reading history: %1 books.", r.history_books),
            r.history_books))
    end
    if #r.existing > 0 then
        local n = #r.existing
        table.insert(lines, T(N_("%1 file already in Kaia, not replaced.", "%1 files already in Kaia, not replaced.", n), n))
    end
    if #r.too_large > 0 then
        table.insert(lines, T(_("Skipped, too large: %1"), tooLargeText(r.too_large)))
    end
    if #r.failed > 0 then
        local names = {}
        for _i, item in ipairs(r.failed) do
            table.insert(names, item.rel)
        end
        table.insert(lines, T(_("Failed: %1"), listText(names)))
    end
    notCopiedLines(r.not_copied, lines)
    notCopiedLines(r.not_copied, lines)
    table.insert(lines, _("KOReader's files were not changed."))
    table.insert(lines, _("Restart Kaia to load the imported data."))
    return table.concat(lines, "\n")
end

-- Shows the plan, then runs the import on OK and shows the report with a
-- restart option.
local function confirmAndRun(source, dest, cancel_text)
    local opts = { settings = G_reader_settings }
    local plan = ImportKO.plan(source, dest, opts)
    UIManager:show(ConfirmBox:new{
        text = ImportKO.formatPlan(plan),
        ok_text = _("Import"),
        cancel_text = cancel_text,
        ok_callback = function()
            local report = ImportKO.run(source, dest, opts)
            UIManager:show(ConfirmBox:new{
                text = ImportKO.formatReport(report),
                ok_text = _("Restart"),
                cancel_text = _("Later"),
                ok_callback = function()
                    UIManager:askForRestart()
                end,
            })
        end,
    })
end

--- Menu entry: finds KOReader's data, then asks before importing.
function ImportKO.showMenu()
    local dest = DataStorage:getDataDir()
    local source = ImportKO.findSource{ dest = dest }
    if not source then
        UIManager:show(InfoMessage:new{ text = _("No KOReader data found next to Kaia.") })
        return
    end
    confirmAndRun(source, dest, _("Cancel"))
end

--- Offers the import once, on Home, when Kaia has no reading history.
-- The offer is made only when KOReader data exists. Once it is shown, the
-- flag is set whatever the answer is.
-- @treturn bool true when the offer was shown
function ImportKO.offerOnce()
    if G_reader_settings:isTrue(OFFER_FLAG) then
        return false
    end
    local dest = DataStorage:getDataDir()
    local source = ImportKO.findSource{ dest = dest }
    if not source then
        return false
    end
    if #require("readhistory").hist > 0 then
        return false
    end
    G_reader_settings:makeTrue(OFFER_FLAG)
    G_reader_settings:flush()
    confirmAndRun(source, dest, _("Not now"))
    return true
end

return ImportKO
