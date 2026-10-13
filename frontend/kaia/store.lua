--[[--
Sticker store for Kaia, a Lua class over one libkaia store handle.

Store.open(dir) returns a store, or nil and a message. Any method that can fail
returns nil, code, message. The code is a RustLib.ERR value and the message is
the text from kaia_last_error. Methods that return a value return it, and the
ones that only change state return true.

Records are Lua tables. JSON goes through rapidjson, and null values in
decoded records become nil. Every kaia_buf is freed after its bytes are
copied, and a finalizer frees any buffer that was never copied.

Store.quoteLocate needs no store. Store.defaultDir() is the directory KOReader
uses for the store.

@module kaia.store
--]]

local DataStorage = require("datastorage")
local RustLib = require("kaia/rustlib")
local ffi = require("ffi")
local rapidjson = require("rapidjson")

local OK = RustLib.ERR.OK

local Store = {}
Store.__index = Store

-- One finalizer per library object, so each buffer does not need a new closure.
local finalizers = setmetatable({}, { __mode = "k" })

local function finalizerFor(lib)
    local free = finalizers[lib]
    if not free then
        free = function(buf)
            lib.kaia_buf_free(buf)
        end
        finalizers[lib] = free
    end
    return free
end

-- A zeroed kaia_buf that libkaia fills. The finalizer frees it if no caller does.
local function newBuf(lib)
    return ffi.gc(ffi.new("kaia_buf[1]"), finalizerFor(lib))
end

-- Copies the bytes out, then frees the buffer. Returns "" for an empty buffer.
local function takeBuf(lib, buf)
    local data = buf[0]
    local text = ""
    if data.len > 0 then
        text = ffi.string(data.ptr, data.len)
    end
    lib.kaia_buf_free(buf)
    return text
end

-- The message of the last error on this thread.
local function lastError(lib)
    local buf = newBuf(lib)
    local rc = lib.kaia_last_error(buf)
    local text = takeBuf(lib, buf)
    if rc ~= OK then
        return "unknown libkaia error"
    end
    return text
end

local function fail(lib, rc)
    return nil, rc, lastError(lib)
end

-- Checks a call that returns no data.
local function status(lib, rc)
    if rc ~= OK then
        return fail(lib, rc)
    end
    return true
end

-- Turns a null into nil, in place, so records do not carry rapidjson.null.
local function denull(value)
    if type(value) ~= "table" then
        return value
    end
    for key, item in pairs(value) do
        if item == rapidjson.null then
            value[key] = nil
        elseif type(item) == "table" then
            denull(item)
        end
    end
    return value
end

local function decodeJson(text)
    local ok, value = pcall(rapidjson.decode, text)
    if not ok then
        return nil, RustLib.ERR.DB, "libkaia returned invalid JSON: " .. tostring(value)
    end
    return denull(value)
end

-- Checks a call that returns a buffer, copies it out, and decodes it when asked.
-- `decode` maps the text to the value, or to nil plus code and message.
local function finish(lib, rc, buf, decode)
    local text = takeBuf(lib, buf)
    if rc ~= OK then
        return fail(lib, rc)
    end
    if decode then
        return decode(text)
    end
    return text
end

-- Passes a Lua string to a `const uint8_t *` parameter.
local function bytesOf(text)
    return ffi.cast("const uint8_t *", text), #text
end

--- The directory of the store inside KOReader's data directory.
function Store.defaultDir()
    return DataStorage:getDataDir() .. "/kaia"
end

--- Opens or creates the store in `dir`. Returns the store, or nil and a message.
function Store.open(dir)
    local lib, reason = RustLib.load()
    if not lib then
        return nil, reason
    end
    local handle = ffi.new("uint64_t[1]")
    local rc = lib.kaia_store_open(dir, handle)
    if rc ~= OK then
        return nil, lastError(lib)
    end
    return setmetatable({ lib = lib, handle = handle[0] }, Store)
end

--- Closes the store. Later calls on this object fail with BAD_HANDLE.
function Store:close()
    return status(self.lib, self.lib.kaia_store_close(self.handle))
end

--- Inserts a sticker when `record` has no id, otherwise updates it. Returns the stored record.
function Store:put(record)
    local lib = self.lib
    local json, len = bytesOf(rapidjson.encode(record))
    local buf = newBuf(lib)
    local rc = lib.kaia_sticker_put(self.handle, json, len, buf)
    return finish(lib, rc, buf, decodeJson)
end

--- Returns the sticker with this id.
function Store:get(id)
    local lib = self.lib
    local buf = newBuf(lib)
    local rc = lib.kaia_sticker_get(self.handle, id, buf)
    return finish(lib, rc, buf, decodeJson)
end

--- Returns the stickers of one document, oldest first. An unknown document gives {}.
function Store:list(partial_md5)
    local lib = self.lib
    local buf = newBuf(lib)
    local rc = lib.kaia_sticker_list(self.handle, partial_md5, buf)
    return finish(lib, rc, buf, decodeJson)
end

--- Returns every sticker, grouped by document.
function Store:listAll()
    local lib = self.lib
    local buf = newBuf(lib)
    local rc = lib.kaia_sticker_list_all(self.handle, buf)
    return finish(lib, rc, buf, decodeJson)
end

--- Deletes a sticker. Its asset file stays until gc().
function Store:delete(id)
    return status(self.lib, self.lib.kaia_sticker_delete(self.handle, id))
end

--- Sets the state to "attached", "needs_review" or "orphaned".
function Store:setState(id, state)
    return status(self.lib, self.lib.kaia_sticker_set_state(self.handle, id, state))
end

--- Copies a PNG or JPEG into the store. Returns the asset record.
-- `license` is free text, and nil means empty.
function Store:addAsset(path, license)
    local lib = self.lib
    local buf = newBuf(lib)
    local rc = lib.kaia_asset_add_file(self.handle, path, license or "", buf)
    return finish(lib, rc, buf, decodeJson)
end

--- Returns the file path of an asset, given its sha256.
function Store:assetPath(sha256)
    local lib = self.lib
    local buf = newBuf(lib)
    local rc = lib.kaia_asset_path(self.handle, sha256, buf)
    return finish(lib, rc, buf)
end

--- Removes assets that no sticker uses. Returns the number removed.
function Store:gc()
    local lib = self.lib
    local removed = ffi.new("uint32_t[1]")
    local rc = lib.kaia_store_gc(self.handle, removed)
    if rc ~= OK then
        return fail(lib, rc)
    end
    return tonumber(removed[0])
end

--- Records that a document was opened. Returns true when the stored file size
-- differed, which moves its attached stickers to "needs_review".
function Store:documentSeen(partial_md5, file_size, title)
    local lib = self.lib
    local changed = ffi.new("int32_t[1]")
    local rc = lib.kaia_document_seen(self.handle, partial_md5, file_size, title or "", changed)
    if rc ~= OK then
        return fail(lib, rc)
    end
    return changed[0] == 1
end

--- Writes stickers and assets to a new export directory. Returns its path.
function Store:export()
    local lib = self.lib
    local buf = newBuf(lib)
    local rc = lib.kaia_export(self.handle, buf)
    return finish(lib, rc, buf)
end

--- Imports a directory written by export(). Returns the report table, with
-- stickers_added, stickers_skipped and assets_added.
function Store:import(dir)
    local lib = self.lib
    local buf = newBuf(lib)
    local rc = lib.kaia_import(self.handle, dir, buf)
    return finish(lib, rc, buf, decodeJson)
end

--- Finds the quote in the text. Returns the 0-based character index, or nil when
-- there is no match. For failures it returns nil, code, message. Without the
-- library it returns nil and the reason.
function Store.quoteLocate(haystack, exact, prefix, suffix)
    local lib, reason = RustLib.load()
    if not lib then
        return nil, reason
    end
    local index = ffi.new("int64_t[1]")
    local rc = lib.kaia_quote_locate(haystack, exact, prefix or "", suffix or "", index)
    if rc ~= OK then
        return fail(lib, rc)
    end
    if index[0] < 0 then
        return nil
    end
    return tonumber(index[0])
end

return Store
