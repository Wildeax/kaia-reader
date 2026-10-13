-- Development user patch used by tools/kaia/screenshot.sh. It runs an optional
-- Lua action, waits, saves a screenshot and quits. It does nothing unless
-- KAIA_SHOT_OUT is set.
local out = os.getenv("KAIA_SHOT_OUT")
if not out then return end
local UIManager = require("ui/uimanager")
local Screen = require("device").screen
local action = os.getenv("KAIA_SHOT_ACTION")
local delay = tonumber(os.getenv("KAIA_SHOT_DELAY") or "3")
UIManager:scheduleIn(1, function()
    if action and action ~= "" then
        local ok, err = pcall(dofile, action)
        if not ok then
            io.stderr:write("kaia-shot action failed: ", tostring(err), "\n")
        end
    end
    UIManager:scheduleIn(delay, function()
        UIManager:forceRePaint()
        Screen:shot(out)
        UIManager:quit()
    end)
end)
