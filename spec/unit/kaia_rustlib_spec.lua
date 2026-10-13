--[[--
Tests for the libkaia binding: frontend/kaia/rustlib.lua and frontend/kaia/store.lua.

Before running, build the library for the emulator:

    tools/kaia/build-libkaia.sh host

The spec sets KAIA_LIBKAIA to <repo>/rust/target/release/libkaia.<ext>. The
repo root is KAIA_REPO_ROOT when that variable is set, otherwise ../.. from the
working directory. kodev runs busted from koreader-emulator-*/koreader, so ../..
is the repo root. Lua cannot set environment variables, so the spec calls the C
setenv through LuaJIT FFI.
]]

local MD5 = "0123456789abcdef0123456789abcdef"
local OTHER_MD5 = "fedcba9876543210fedcba9876543210"

-- 1x1 red PNG, 69 bytes.
local PNG_RED = string.char(
    137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 2, 0, 0, 0, 144, 119, 83, 222, 0, 0, 0, 12, 73, 68, 65, 84, 120, 156, 99, 248, 207, 192, 0, 0, 3, 1, 1, 0, 201, 254, 146, 239, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130
)

local function repoRoot()
    return os.getenv("KAIA_REPO_ROOT") or "../.."
end

describe("kaia rustlib", function()
    local ffi, lfs, DataStorage
    local RustLib, Store
    local lib_path
    local base_dir
    local counter = 0

    local function setenv(name, value)
        if value == nil then
            ffi.C.unsetenv(name)
        else
            assert(ffi.C.setenv(name, value, 1) == 0)
        end
    end

    local function removeTree(path)
        local attr = lfs.attributes(path)
        if not attr then
            return
        end
        if attr.mode == "directory" then
            for entry in lfs.dir(path) do
                if entry ~= "." and entry ~= ".." then
                    removeTree(path .. "/" .. entry)
                end
            end
            lfs.rmdir(path)
        else
            os.remove(path)
        end
    end

    -- A new, empty store directory for each test.
    local function freshDir()
        counter = counter + 1
        local dir = base_dir .. "/store-" .. counter
        removeTree(dir)
        return dir
    end

    local function sticker(md5, exact)
        return {
            doc = { partial_md5 = md5, file_size = 1000, title = "Test book" },
            anchor = {
                type = "reflowable",
                locator = "/body/p[2]",
                quote = { exact = exact, prefix = "a ", suffix = " b" },
                offset = { dx = 0, dy = -1.2 },
            },
            text = "note",
            size = { w = 0.1, h = 0.07 },
            rotation = 0,
            state = "attached",
        }
    end

    setup(function()
        require("commonrequire")
        ffi = require("ffi")
        require("ffi/posix_h")
        lfs = require("libs/libkoreader-lfs")
        DataStorage = require("datastorage")
        base_dir = DataStorage:getDataDir() .. "/kaia_rustlib_spec"
        lfs.mkdir(base_dir)

        local ext = ffi.os == "OSX" and "dylib" or "so"
        lib_path = repoRoot() .. "/rust/target/release/libkaia." .. ext
        assert(lfs.attributes(lib_path), "missing " .. lib_path .. ", run tools/kaia/build-libkaia.sh host first")
    end)

    before_each(function()
        RustLib = require("kaia/rustlib")
        Store = require("kaia/store")
        setenv("KAIA_LIBKAIA", lib_path)
        RustLib._reset()
    end)

    teardown(function()
        removeTree(base_dir)
        setenv("KAIA_LIBKAIA", nil)
        RustLib._reset()
    end)

    it("loads the library at the ABI version", function()
        local lib = assert(RustLib.load())
        assert.are.equal(1, lib.kaia_abi_version())
    end)

    it("caches the loaded library", function()
        local first = assert(RustLib.load())
        assert.are.equal(first, RustLib.load())
    end)

    it("round trips a sticker through put, get, list and delete", function()
        local store = assert(Store.open(freshDir()))
        local stored = assert(store:put(sticker(MD5, "quoted text")))
        assert.is_string(stored.id)
        assert.are.equal("attached", stored.state)
        assert.are.equal(MD5, stored.doc.partial_md5)
        assert.is_true(stored.created_at > 0)
        assert.is_nil(stored.asset, "a JSON null becomes nil")

        local got = assert(store:get(stored.id))
        assert.are.equal(stored.id, got.id)
        assert.are.equal("quoted text", got.anchor.quote.exact)

        assert.are.equal(1, #assert(store:list(MD5)))
        assert.are.equal(1, #assert(store:listAll()))
        assert.are.same({}, assert(store:list(OTHER_MD5)))

        assert.is_true(store:delete(stored.id))
        local value, code, message = store:get(stored.id)
        assert.is_nil(value)
        assert.are.equal(RustLib.ERR.NOT_FOUND, code)
        assert.is_string(message)
        assert.is_true(store:close())
    end)

    it("updates a sticker, changes its state and reports a changed document", function()
        local store = assert(Store.open(freshDir()))
        local first = assert(store:put(sticker(MD5, "first")))
        local second = assert(store:put(sticker(MD5, "second")))

        first.anchor.quote.exact = "changed"
        local updated = assert(store:put(first))
        assert.are.equal(first.id, updated.id)
        assert.are.equal("changed", assert(store:get(first.id)).anchor.quote.exact)

        assert.is_true(store:setState(first.id, "orphaned"))
        assert.are.equal("orphaned", assert(store:get(first.id)).state)

        assert.is_false(store:documentSeen(MD5, 1000, "Test book"), "same size is not a change")
        assert.is_true(store:documentSeen(MD5, 999, "Test book"), "new size is a change")
        assert.are.equal("needs_review", assert(store:get(second.id)).state)
        assert.are.equal("orphaned", assert(store:get(first.id)).state, "only attached stickers move")
        assert.is_true(store:close())
    end)

    it("names the field in validation errors", function()
        local store = assert(Store.open(freshDir()))
        local bad = sticker(MD5, "quote")
        bad.size.w = 2
        local value, code, message = store:put(bad)
        assert.is_nil(value)
        assert.are.equal(RustLib.ERR.VALIDATION, code)
        assert.truthy(message:find("size.w", 1, true), message)
        assert.is_true(store:close())
    end)

    it("rejects a closed handle", function()
        local store = assert(Store.open(freshDir()))
        assert.is_true(store:close())

        local value, code, message = store:list(MD5)
        assert.is_nil(value)
        assert.are.equal(RustLib.ERR.BAD_HANDLE, code)
        assert.is_string(message)

        local ok, close_code = store:close()
        assert.is_nil(ok)
        assert.are.equal(RustLib.ERR.BAD_HANDLE, close_code)
    end)

    it("locates a quote by character index", function()
        assert.are.equal(16, Store.quoteLocate("Alpha beta. The needle sits here.", "needle", "The ", " sits"))
        assert.are.equal(4, Store.quoteLocate("éèà target here", "target", "à ", " here"))
        assert.is_nil(Store.quoteLocate("Alpha beta.", "needle", "", ""))
        assert.is_nil(Store.quoteLocate("Alpha beta.", "", "", ""))
    end)

    it("exports a store and imports it into an empty store", function()
        local source = assert(Store.open(freshDir()))
        local first = assert(source:put(sticker(MD5, "first")))
        local second = assert(source:put(sticker(MD5, "second")))
        local export_dir = assert(source:export())

        local target = assert(Store.open(freshDir()))
        local report = assert(target:import(export_dir))
        assert.are.equal(2, report.stickers_added)
        assert.are.equal(0, report.stickers_skipped)

        local quotes = {}
        for _, item in ipairs(assert(target:list(MD5))) do
            quotes[item.id] = item.anchor.quote.exact
        end
        assert.are.equal("first", quotes[first.id])
        assert.are.equal("second", quotes[second.id])

        local again = assert(target:import(export_dir))
        assert.are.equal(0, again.stickers_added)
        assert.are.equal(2, again.stickers_skipped)

        assert.is_true(source:close())
        assert.is_true(target:close())
    end)

    it("returns nil and a reason, without raising, when the library is missing", function()
        setenv("KAIA_LIBKAIA", base_dir .. "/no-such-libkaia.dylib")
        RustLib._reset()

        local lib, reason = RustLib.load()
        assert.is_nil(lib)
        assert.is_string(reason)
        assert.truthy(reason:find("no-such-libkaia", 1, true), reason)

        local store, err = Store.open(freshDir())
        assert.is_nil(store)
        assert.is_string(err)
        assert.is_nil(Store.quoteLocate("abc", "b", "", ""))
    end)

    it("adds an image, finds its file and collects it once no sticker uses it", function()
        local dir = freshDir()
        local store = assert(Store.open(dir))
        local png_path = dir .. "/red.png"
        local file = assert(io.open(png_path, "wb"))
        file:write(PNG_RED)
        file:close()

        local asset = assert(store:addAsset(png_path, "CC0"))
        assert.are.equal("image/png", asset.mime)
        assert.are.equal(1, asset.width)
        assert.are.equal("CC0", asset.license)
        assert.truthy(assert(store:assetPath(asset.sha256)):match("%.png$"))

        local record = sticker(MD5, "with image")
        record.asset = asset.sha256
        local stored = assert(store:put(record))
        assert.are.equal(asset.sha256, stored.asset)
        assert.are.equal(0, assert(store:gc()), "a referenced asset is kept")

        assert.is_true(store:delete(stored.id))
        assert.are.equal(1, assert(store:gc()))
        local value, code = store:assetPath(asset.sha256)
        assert.is_nil(value)
        assert.are.equal(RustLib.ERR.NOT_FOUND, code)
        assert.is_true(store:close())
    end)

    it("uses the data directory for the default store", function()
        assert.are.equal(DataStorage:getDataDir() .. "/kaia", Store.defaultDir())
    end)

    it("matches the error codes in kaia.h", function()
        local file = assert(io.open(repoRoot() .. "/rust/kaia-ffi/include/kaia.h", "r"))
        local header = file:read("*a")
        file:close()

        local seen = 0
        for name, value in header:gmatch("#define%s+KAIA_([%w_]+)%s+(%-?%d+)") do
            local key = name == "OK" and "OK" or name:match("^E_(.+)$")
            assert.is_not_nil(key, name)
            assert.are.equal(tonumber(value), RustLib.ERR[key], name)
            seen = seen + 1
        end
        local known = 0
        for _ in pairs(RustLib.ERR) do
            known = known + 1
        end
        assert.are.equal(known, seen)
    end)
end)
