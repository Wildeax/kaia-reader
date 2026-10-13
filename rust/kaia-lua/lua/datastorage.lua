--[[--
Kaia shim for KOReader's datastorage module.

KOReader's module picks the data directory from KO_HOME and the platform. The
Kaia host sets the directory in KAIA_PATHS instead, so each host has its own
data folder and no process environment changes. Every other function comes
from KOReader's file, loaded unchanged.
]]

-- luacheck: globals KAIA_PATHS

local DataStorage = dofile(KAIA_PATHS.koreader .. "/datastorage.lua")

function DataStorage:getDataDir()
    return KAIA_PATHS.data
end

return DataStorage
