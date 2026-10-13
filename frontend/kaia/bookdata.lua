--[[--
Data helpers for Kaia Home. No widgets.

Entries come from ReadHistory. Metadata comes from the book's DocSettings
sidecar and, when the coverbrowser plugin is loaded, from its BookInfo cache.
A document is never opened with DocumentRegistry, so a missing or corrupt book
cannot slow Home down or start a render.

Both functions take an optional `opts` table so tests can inject:
- `history`: list of ReadHistory-like entries (`file`, `dim`).
- `metadata`: function(file) returning a table with `title`, `authors`,
  `percent`, `status`, `cover_bb`. Missing keys are fine.
- `exists`: function(file) returning true when the file is on disk.

@module kaia.bookdata
]]

local DocSettings = require("docsettings")
local lfs = require("libs/libkoreader-lfs")

local BookData = {}

local function defaultHistory()
    return require("readhistory").hist
end

local function fileExists(file)
    return lfs.attributes(file, "mode") == "file"
end

local function nonEmpty(value)
    if type(value) == "string" and value ~= "" then
        return value
    end
    return nil
end

--- Returns the file name without directories or extension.
function BookData.baseTitle(file)
    local name = file:match("([^/]+)$") or file
    return (name:gsub("%.[^.]+$", ""))
end

--- Reads metadata for one book without opening the document.
-- Sidecar values come from DocSettings. Title, authors and cover come from the
-- coverbrowser BookInfo cache, but only when that plugin has been loaded.
-- BookInfoManager reads its SQLite cache and never extracts metadata itself.
function BookData.readMetadata(file)
    local meta = {}
    if DocSettings:hasSidecarFile(file) then
        local doc_settings = DocSettings:open(file)
        meta.percent = doc_settings:readSetting("percent_finished")
        local summary = doc_settings:readSetting("summary")
        meta.status = summary and summary.status
        local props = doc_settings:readSetting("doc_props")
        if props then
            meta.title = nonEmpty(props.title)
            meta.authors = nonEmpty(props.authors)
        end
    end
    -- package.loaded only has the module when the coverbrowser plugin is active.
    local BookInfoManager = package.loaded["bookinfomanager"]
    if BookInfoManager and BookInfoManager.getBookInfo then
        local info = BookInfoManager:getBookInfo(file, true)
        if info then
            meta.title = nonEmpty(info.title) or meta.title
            meta.authors = nonEmpty(info.authors) or meta.authors
            meta.cover_bb = info.cover_bb
        end
    end
    return meta
end

--- Returns up to `n` books from history that still exist, newest first.
-- @treturn table list of { file, title, authors, percent, status, cover_bb }
-- percent is 0..1 or nil. Title falls back to the file name without extension.
function BookData.recent(n, opts)
    opts = opts or {}
    local history = opts.history or defaultHistory()
    local metadata = opts.metadata or BookData.readMetadata
    local exists = opts.exists or fileExists
    local books = {}
    for _, item in ipairs(history) do
        if #books >= n then
            break
        end
        if not item.dim and item.file and exists(item.file) then
            local meta = metadata(item.file) or {}
            books[#books + 1] = {
                file = item.file,
                title = nonEmpty(meta.title) or BookData.baseTitle(item.file),
                authors = nonEmpty(meta.authors),
                percent = type(meta.percent) == "number" and meta.percent or nil,
                status = meta.status,
                cover_bb = meta.cover_bb,
            }
        end
    end
    return books
end

--- Returns summary numbers for Home.
-- @treturn table { books_in_history = n }, counting history entries on disk
function BookData.counts(opts)
    opts = opts or {}
    local history = opts.history or defaultHistory()
    local exists = opts.exists or fileExists
    local n = 0
    for _, item in ipairs(history) do
        if not item.dim and item.file and exists(item.file) then
            n = n + 1
        end
    end
    return { books_in_history = n }
end

return BookData
