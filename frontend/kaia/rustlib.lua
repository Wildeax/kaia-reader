--[[--
Loads libkaia, the Rust library behind Kaia features.

RustLib.load() returns the library object. When the library cannot be used it
returns nil and a reason. Callers treat nil as "feature off". Without the
library KOReader behaves as the baseline.

Search order:

1. The KAIA_LIBKAIA environment variable, a full path. When it is set, it is
   the only candidate.
2. libs/libkaia.so and libs/libkaia.dylib, relative to the working directory.
   KOReader runs from its install directory, so these sit next to the binary.

A library is accepted only when kaia_abi_version() returns RustLib.ABI_VERSION.
The result is cached, failures included. RustLib._reset() clears the cache and
exists for tests.

@module kaia.rustlib
--]]

local ffi = require("ffi")
local logger = require("logger")

local RustLib = {}

RustLib.ABI_VERSION = 1

-- Return codes from rust/kaia-ffi/include/kaia.h. The names drop the KAIA_E_
-- prefix. KAIA_OK is OK.
RustLib.ERR = {
    OK = 0,
    INVALID_ARG = -1,
    BAD_HANDLE = -2,
    NOT_FOUND = -3,
    IO = -4,
    DB = -5,
    SCHEMA_TOO_NEW = -6,
    VALIDATION = -7,
    ASSET_REJECTED = -8,
    PANIC = -99,
}

-- { lib = library } or { reason = string }, nil until the first load.
local cached

local function exists(path)
    local f = io.open(path, "rb")
    if f then
        f:close()
        return true
    end
    return false
end

local function candidates()
    local env = os.getenv("KAIA_LIBKAIA")
    if env and env ~= "" then
        return { env }
    end
    return { "libs/libkaia.so", "libs/libkaia.dylib" }
end

-- Returns the library and its path, or nil and a reason. Never raises.
local function loadFirst()
    local paths = candidates()
    local path
    for _, candidate in ipairs(paths) do
        if exists(candidate) then
            path = candidate
            break
        end
    end
    if not path then
        return nil, "libkaia not found: " .. table.concat(paths, ", ")
    end

    local declared, decl_err = pcall(require, "kaia/ffi/kaia_h")
    if not declared then
        return nil, "cannot declare the libkaia ABI: " .. tostring(decl_err)
    end

    local loaded, lib = pcall(ffi.load, path)
    if not loaded then
        return nil, "cannot load " .. path .. ": " .. tostring(lib)
    end

    local read, version = pcall(function()
        return lib.kaia_abi_version()
    end)
    if not read then
        return nil, path .. " has no kaia_abi_version: " .. tostring(version)
    end
    if version ~= RustLib.ABI_VERSION then
        return nil, string.format("%s has ABI version %s, expected %d", path, tostring(version), RustLib.ABI_VERSION)
    end
    return lib, path
end

--- Returns the libkaia library object, or nil and a reason.
function RustLib.load()
    if not cached then
        local lib, info = loadFirst()
        if lib then
            logger.info("kaia: loaded Rust library", info)
            cached = { lib = lib }
        else
            logger.warn("kaia: Rust library unavailable:", info)
            cached = { reason = info }
        end
    end
    if cached.lib then
        return cached.lib
    end
    return nil, cached.reason
end

--- Clears the cached result so the next load() searches again. Tests only.
function RustLib._reset()
    cached = nil
end

return RustLib
