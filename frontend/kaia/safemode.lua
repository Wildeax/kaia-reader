--[[--
Kaia safe mode.

Safe mode skips third-party plugins and user patches so a bad extension
cannot keep Kaia from starting. It turns on when any of these is true:

- the environment variable KAIA_SAFE_MODE is "1";
- the file `<data dir>/safemode` exists (a user can create it over USB);
- the last three startups did not reach a stable running state.

Each startup increments a counter in `<data dir>/kaia-startup-attempts`.
The counter resets once Kaia has run for `stable_after` seconds, or on a
clean exit. This module only uses the standard library and lfs because it
runs before settings and the UI exist.

@module kaia.safemode
]]

local lfs = require("libs/libkoreader-lfs")

local SafeMode = {
    threshold = 3,
    stable_after = 15,
    active = false,
    reason = nil,
    attempts = 0,
    dir = nil,
}

local function readNumber(path)
    local f = io.open(path, "r")
    if not f then return 0 end
    local n = tonumber(f:read("*l") or "")
    f:close()
    return n or 0
end

local function writeNumber(path, n)
    local f = io.open(path, "w")
    if not f then return false end
    f:write(tostring(n), "\n")
    f:close()
    return true
end

function SafeMode:counterPath() return self.dir .. "/kaia-startup-attempts" end
function SafeMode:flagPath() return self.dir .. "/safemode" end

--- Records a startup attempt and decides whether safe mode is on.
-- @string data_dir the Kaia data directory
-- @tparam[opt] function getenv for tests, defaults to os.getenv
function SafeMode:init(data_dir, getenv)
    getenv = getenv or os.getenv
    self.dir = data_dir
    self.attempts = readNumber(self:counterPath()) + 1
    writeNumber(self:counterPath(), self.attempts)
    if getenv("KAIA_SAFE_MODE") == "1" then
        self.reason = "env"
    elseif lfs.attributes(self:flagPath(), "mode") == "file" then
        self.reason = "flag"
    elseif self.attempts > self.threshold then
        self.reason = "failed_startups"
    else
        self.reason = nil
    end
    self.active = self.reason ~= nil
    return self.active
end

--- Marks this startup as stable, so the failed-start counter resets.
function SafeMode:markStable()
    if self.dir then
        writeNumber(self:counterPath(), 0)
    end
end

--- Leaves safe mode for the next start: removes the flag and resets the counter.
function SafeMode:clear()
    if not self.dir then return end
    os.remove(self:flagPath())
    writeNumber(self:counterPath(), 0)
end

--- True when a plugin should be skipped because safe mode is on.
-- @string plugin_name plugin directory name without ".koplugin"
-- @tparam table builtin set of bundled plugin names
function SafeMode:skipsPlugin(plugin_name, builtin)
    return self.active and not builtin[plugin_name]
end

--- Called once the UI is up: schedules the stable mark and tells the user
-- when safe mode is on.
function SafeMode:afterStartup()
    local UIManager = require("ui/uimanager")
    UIManager:scheduleIn(self.stable_after, function() self:markStable() end)
    if not self.active then return end
    local ConfirmBox = require("ui/widget/confirmbox")
    local _ = require("gettext")
    local why = {
        env = _("Safe mode was requested at launch."),
        flag = _("A safemode file is present in the Kaia folder."),
        failed_startups = _("Kaia did not start correctly several times in a row."),
    }
    UIManager:show(ConfirmBox:new{
        text = (why[self.reason] or "") .. "\n\n"
            .. _("Kaia started in safe mode. Third-party plugins and user patches are off.\n\nLeave safe mode the next time Kaia starts?"),
        ok_text = _("Leave safe mode"),
        cancel_text = _("Stay in safe mode"),
        ok_callback = function()
            self:clear()
            UIManager:askForRestart(_("Kaia will start normally after a restart."))
        end,
    })
end

return SafeMode
