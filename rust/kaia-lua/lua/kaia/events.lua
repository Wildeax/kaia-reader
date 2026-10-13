--[[--
Builds KOReader event objects from values that the Rust side sends.

Rust sends plain tables with numbers and strings, because it cannot build
KOReader's Geom, time or Key objects. This module turns them into the objects
that KOReader's widgets expect.
]]

local Event = require("ui/event")
local Geom = require("ui/geometry")
local Key = require("device/key")
local time = require("ui/time")

local M = {}

local function point(p)
    if p == nil then
        return nil
    end
    return Geom:new{ x = p.x, y = p.y }
end

-- `Event:new("Gesture", ges)` with the fields of `frontend/device/gesturedetector.lua`.
-- The Rust table holds `time_us`, which becomes the `time` field.
function M.gesture(t)
    local ges = {}
    for k, v in pairs(t) do
        ges[k] = v
    end
    ges.pos = point(t.pos)
    ges.start_pos = point(t.start_pos)
    ges.end_pos = point(t.end_pos)
    ges.time = time.us(t.time_us)
    ges.time_us = nil
    return Event:new("Gesture", ges)
end

-- `KeyPress` or `KeyRelease` with a device key object, as `frontend/device/input.lua` sends them.
function M.key(name, pressed, shift)
    return Event:new(pressed and "KeyPress" or "KeyRelease", Key:new(name, shift and {Shift=true} or {}))
end

-- An event with the given name and arguments.
function M.named(name, ...)
    return Event:new(name, ...)
end

return M
