--[[--
Data helpers for Kaia Notes. No widgets.

Highlights, notes and bookmarks come from each book's DocSettings sidecar, the
`annotations` array. The sidecar is read with `dofile` and never written: a
document is not opened, and DocSettings:open() is not used because it removes
candidate files it cannot read. A sidecar that fails to load is skipped.

Kinds:
- "highlight": a highlight without a note.
- "note": a highlight with a note. Its note text is `note`.
- "bookmark": a page bookmark (no `drawer`). Its note text is `note`, if any.

Stickers are an extension point for Kaia stickers. Set `NotesData.stickerProvider`
to a function that returns `{ by_file = { [file] = { entry, ... } }, orphaned = { entry, ... } }`.
Sticker entries have the same fields as `NotesData.entries()` with kind "sticker".
`orphaned` entries also carry `file`. Nothing sets the provider yet.

@module kaia.notesdata
]]

local BookData = require("kaia/bookdata")
local DocSettings = require("docsettings")
local lfs = require("libs/libkoreader-lfs")
local logger = require("logger")

local NotesData = {}

-- Set by the sticker task. Nil means no stickers. See the module comment.
NotesData.stickerProvider = nil

local function defaultHistory()
    return require("readhistory").hist
end

local function nonEmpty(value)
    if type(value) == "string" and value ~= "" then
        return value
    end
    return nil
end

local function fileExists(file)
    return lfs.attributes(file, "mode") == "file"
end

-- Returns the annotations array and the whole sidecar table, or nil when the
-- book has no readable sidecar.
local function loadSidecar(file)
    local path = DocSettings:findSidecarFile(file)
    if not path then
        return nil
    end
    local ok, stored = pcall(dofile, path)
    if not ok or type(stored) ~= "table" then
        logger.warn("kaia notes: skipping unreadable sidecar", path)
        return nil
    end
    if type(stored.annotations) ~= "table" then
        return nil
    end
    return stored.annotations, stored
end

local function kindOf(item)
    if item.drawer then
        return nonEmpty(item.note) and "note" or "highlight"
    end
    return "bookmark"
end

local function entryFrom(item)
    return {
        kind = kindOf(item),
        text = nonEmpty(item.text),
        note = nonEmpty(item.note),
        chapter = nonEmpty(item.chapter),
        datetime = nonEmpty(item.datetime),
        -- What the reader needs for GotoPage or GotoXPointer: page and marker position.
        location = { page = item.page, pos0 = item.pos0 },
    }
end

-- Title and authors from the sidecar's doc_props. The coverbrowser cache is
-- read only when that plugin is loaded, as in kaia/bookdata.
local function titleAndAuthors(file, stored)
    local props = stored.doc_props
    local title = type(props) == "table" and nonEmpty(props.title) or nil
    local authors = type(props) == "table" and nonEmpty(props.authors) or nil
    local BookInfoManager = package.loaded["bookinfomanager"]
    if BookInfoManager and BookInfoManager.getBookInfo then
        local info = BookInfoManager:getBookInfo(file, true)
        if info then
            title = nonEmpty(info.title) or title
            authors = nonEmpty(info.authors) or authors
        end
    end
    return title or BookData.baseTitle(file), authors
end

--- Returns books that have at least one annotation, newest first.
-- @param history list of ReadHistory-like items (`file`, `dim`). Nil uses ReadHistory.
-- @treturn table list of { file, title, authors, count_highlights, count_notes,
--   count_bookmarks, last_datetime }. last_datetime is the newest annotation
--   datetime string, or nil. Equal dates keep history order.
function NotesData.books(history)
    history = history or defaultHistory()
    local books = {}
    local seen = {}
    for index, item in ipairs(history) do
        local file = item.file
        if file and not item.dim and not seen[file] and fileExists(file) then
            seen[file] = true
            local annotations, stored = loadSidecar(file)
            if annotations then
                local book = {
                    file = file,
                    count_highlights = 0,
                    count_notes = 0,
                    count_bookmarks = 0,
                    _index = index,
                }
                for _, item_a in ipairs(annotations) do
                    if type(item_a) == "table" then
                        local kind = kindOf(item_a)
                        if kind == "highlight" then
                            book.count_highlights = book.count_highlights + 1
                        elseif kind == "note" then
                            book.count_notes = book.count_notes + 1
                        else
                            book.count_bookmarks = book.count_bookmarks + 1
                        end
                        local dt = nonEmpty(item_a.datetime)
                        if dt and (not book.last_datetime or dt > book.last_datetime) then
                            book.last_datetime = dt
                        end
                    end
                end
                local total = book.count_highlights + book.count_notes + book.count_bookmarks
                if total > 0 then
                    book.title, book.authors = titleAndAuthors(file, stored)
                    books[#books + 1] = book
                end
            end
        end
    end
    table.sort(books, function(a, b)
        local da, db = a.last_datetime or "", b.last_datetime or ""
        if da ~= db then
            return da > db
        end
        return a._index < b._index
    end)
    for _, book in ipairs(books) do
        book._index = nil
    end
    return books
end

--- Returns the annotations of one book in reading order (the sidecar's array order).
-- @string file book path
-- @treturn table list of { kind, text, note, chapter, datetime, location }
function NotesData.entries(file)
    local result = {}
    local annotations = loadSidecar(file)
    if not annotations then
        return result
    end
    for _, item in ipairs(annotations) do
        if type(item) == "table" then
            result[#result + 1] = entryFrom(item)
        end
    end
    return result
end

--- Returns sticker data from the extension point, or nil when no provider is set.
-- @treturn table|nil `{ by_file = {...}, orphaned = {...} }`
function NotesData.stickers()
    if not NotesData.stickerProvider then
        return nil
    end
    local ok, result = pcall(NotesData.stickerProvider)
    if not ok or type(result) ~= "table" then
        logger.warn("kaia notes: sticker provider failed", result)
        return nil
    end
    return result
end

return NotesData
