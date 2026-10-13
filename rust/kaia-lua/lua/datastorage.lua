--[[--
Kaia shim for KOReader's datastorage module.

KOReader's module picks the data directory from KO_HOME and the platform. The
Kaia host sets the directory in KAIA_PATHS instead, so each host has its own
data folder and no process environment changes. Every other function comes
from KOReader's file, loaded unchanged.

The folders that KOReader expects inside the data directory are created here,
because the cache and history modules write into them without making them.
]]

-- luacheck: globals KAIA_PATHS

local lfs = require("libs/libkoreader-lfs")
local DataStorage = dofile(KAIA_PATHS.koreader .. "/datastorage.lua")

function DataStorage:getDataDir()
    return KAIA_PATHS.data
end

-- The subfolders of the data directory that KOReader's modules write into.
local SUBDIRS = { "cache", "history", "settings", "clipboard", "ota", "dict", "screenshots", "plugins" }

for _, name in ipairs(SUBDIRS) do
    lfs.mkdir(KAIA_PATHS.data .. "/" .. name)
end

return DataStorage
