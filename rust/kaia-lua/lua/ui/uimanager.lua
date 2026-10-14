--[[--
Kaia shim for KOReader's UIManager.

The window stack, the task queue and the refresh queue live in Rust (`LuaUi`).
Each method here keeps the name and the arguments of KOReader's method and
forwards to the table `KAIA_UI`, which the Rust side creates. A show or close
changes what the window queries answer at once. The Rust side creates or removes
the window, and sends its events, after the Lua call that asked for it returns.
Dirty marks and tasks wait for that point too. The differences from KOReader are
listed in `rust/kaia-lua/README.md`.

Widgets keep their identity: the table that `show` receives is the table that
`close` and `setDirty` receive, and the Rust side compares the two by reference.
]]

-- luacheck: globals KAIA_UI G_reader_settings

local Screen = require("device").screen
local logger = require("logger")
local _ = require("gettext")

local UIManager = {
    event_hook = require("ui/hook_container"):new(),
    FULL_REFRESH_COUNT = 6,
    refresh_count = 0,
    currently_scrolling = false,
    silent_mode = nil,
    _input_gestures_disabled = false,
}

-- Hook owners keep errors and old instances out of the next plugin load.
local hook = UIManager.event_hook
local register, unregister = hook.register, hook.unregister
local owned_hooks = setmetatable({}, { __mode = "k" })
function hook:register(name, callback)
    local owner = KAIA_PLUGIN and (KAIA_PLUGIN.owner or KAIA_PLUGIN.loading_name)
    if not owner then return register(self, name, callback) end
    local generation = KAIA_PLUGIN.generation
    local wrapper = function(...)
        if generation == KAIA_PLUGIN.generation then
            return KAIA_PLUGIN.guarded(owner, callback, ...)
        end
    end
    owned_hooks[callback] = wrapper
    return register(self, name, wrapper)
end
function hook:unregister(name, callback)
    return unregister(self, name, owned_hooks[callback] or callback)
end

-- Scheduled calls with arguments are wrapped so the task can run with them.
-- Each action keeps its wrappers, so unschedule can find them.
local wrappers = setmetatable({}, { __mode = "k" })

-- Converts a refresh request to the table that the Rust side reads. Returns
-- nil when there is no refresh type, as KOReader does. A function refresh type
-- becomes `deferred`: KOReader calls it after the widget is painted, and so does
-- the Rust side.
local function refresh_spec(refreshtype, region, dither)
    if refreshtype == nil then
        return nil
    end
    if type(refreshtype) == "function" then
        local owner = KAIA_PLUGIN and KAIA_PLUGIN.owner
        if owner then
            return { deferred = function(...)
                local ok, mode, region, dither = KAIA_PLUGIN.guarded(owner, refreshtype, ...)
                if ok then return mode, region, dither end
            end }
        end
        return { deferred = refreshtype }
    end
    local spec = { mode = refreshtype, dither = dither and true or false }
    if region ~= nil then
        spec.region = { x = region.x, y = region.y, w = region.w, h = region.h }
    end
    return spec
end

local function window_flags(widget)
    return {
        modal = widget.modal and true or false,
        toast = widget.toast and true or false,
        covers_fullscreen = widget.covers_fullscreen and true or false,
        always_active = widget.is_always_active and true or false,
    }
end

function UIManager:init()
    -- KOReader sets up the event handlers and the power actions here. The Kaia
    -- host handles power in Rust, so there is nothing to do.
end

-- KOReader's InputContainer swaps the gesture handler and records the state in
-- UIManager._input_gestures_disabled, so the call goes there.
function UIManager:setIgnoreTouchInput(state)
    local InputContainer = require("ui/widget/container/inputcontainer")
    InputContainer:setIgnoreTouchInput(state)
end

function UIManager:setSilentMode(toggle)
    self.silent_mode = toggle or nil
end

function UIManager:isInSilentMode()
    return self.silent_mode or false
end

function UIManager:show(widget, refreshtype, refreshregion, x, y, refreshdither)
    if not widget then
        logger.dbg("attempted to show a nil widget")
        return
    end
    if self.silent_mode and widget.honor_silent_mode then
        logger.dbg("widget show disabled:", widget.id or widget.name or tostring(widget))
        return
    end
    local owner = KAIA_PLUGIN and KAIA_PLUGIN.owner
    if owner and not rawget(widget, "_kaia_guarded") then
        widget._kaia_guarded = true
        for _, method in ipairs({"handleEvent", "paintTo"}) do
            local fn = widget[method]
            if fn then
                widget[method] = function(...)
                    local ok, result = KAIA_PLUGIN.guarded(owner, fn, ...)
                    if not ok and not rawget(widget, "_kaia_closing_failed") then
                        widget._kaia_closing_failed = true
                        UIManager:close(widget)
                    end
                    return ok and result or false
                end
            end
        end
    end
    KAIA_UI.show(widget, x or 0, y or 0, window_flags(widget),
        refresh_spec(refreshtype, refreshregion, refreshdither))
end

function UIManager:close(widget, refreshtype, refreshregion, refreshdither)
    if not widget then
        logger.dbg("attempted to close a nil widget")
        return
    end
    KAIA_UI.close(widget, refresh_spec(refreshtype, refreshregion, refreshdither))
end

function UIManager:setDirty(widget, refreshtype, refreshregion, refreshdither)
    local spec = refresh_spec(refreshtype, refreshregion, refreshdither)
    if widget == "all" then
        KAIA_UI.set_dirty(nil, true, false, spec)
    elseif widget == nil then
        KAIA_UI.set_dirty(nil, false, false, spec)
    else
        KAIA_UI.set_dirty(widget, false, widget.invisible and true or false, spec)
    end
end

function UIManager:scheduleIn(seconds, action, ...)
    if action == nil then
        logger.dbg("scheduleIn without an action")
        return
    end
    local owner = KAIA_PLUGIN and KAIA_PLUGIN.owner
    if not owner and select("#", ...) == 0 then
        KAIA_UI.schedule(seconds, action)
        return
    end
    local generation = KAIA_PLUGIN and KAIA_PLUGIN.generation
    local args = table.pack(...)
    local wrapper
    wrapper = function()
        if generation and generation ~= KAIA_PLUGIN.generation then return end
        if KAIA_DEVICE and KAIA_DEVICE("reader_busy") then
            KAIA_UI.schedule(0.02, wrapper)
            return
        end
        if owner then
            local ok, result = KAIA_PLUGIN.guarded(owner, action, unpack(args, 1, args.n))
            if ok then return result end
        else
            return action(unpack(args, 1, args.n))
        end
    end
    wrappers[action] = wrappers[action] or {}
    table.insert(wrappers[action], wrapper)
    KAIA_UI.schedule(seconds, wrapper)
end

function UIManager:nextTick(action, ...)
    return self:scheduleIn(0, action, ...)
end

-- The same structure as KOReader: the first tick schedules the action for the
-- tick after it. The returned wrapper is the handle for unschedule.
function UIManager:tickAfterNext(action, ...)
    local action_wrapper = function(...)
        self:nextTick(action, ...)
    end
    self:nextTick(action_wrapper, ...)
    return action_wrapper
end

function UIManager:unschedule(action)
    KAIA_UI.unschedule(action)
    local list = wrappers[action]
    if list then
        for _, wrapper in ipairs(list) do
            KAIA_UI.unschedule(wrapper)
        end
        wrappers[action] = nil
    end
    return true
end

function UIManager:preventStandby()
    self._standby_holds = (self._standby_holds or 0) + 1
end
function UIManager:allowStandby()
    self._standby_holds = math.max(0, (self._standby_holds or 0) - 1)
end
function UIManager:getElapsedTimeSinceBoot()
    return require("ui/time").boottime_or_realtime_coarse()
end

function UIManager:getTime()
    return require("ui/time").now()
end

function UIManager:sendEvent(event)
    KAIA_UI.send(event, false)
end

function UIManager:broadcastEvent(event)
    KAIA_UI.send(event, true)
end

function UIManager:getTopmostVisibleWidget()
    return KAIA_UI.topmost_visible() or (KAIA_PLUGIN and KAIA_PLUGIN.ui)
end

function UIManager:isWidgetShown(widget)
    return KAIA_UI.is_shown(widget)
end

function UIManager:getNthTopWidget(n)
    return KAIA_UI.nth_top(n or 1)
end

function UIManager:forceRePaint()
    KAIA_UI.force_repaint()
end

-- Paints a widget into the screen buffer now, without a refresh. The caller
-- queues the refresh with setDirty, as KOReader's callers do.
function UIManager:widgetRepaint(widget, x, y)
    if not widget then
        return
    end
    widget:paintTo(Screen.bb, x, y)
end

-- Inverts a rectangle of the screen buffer now, without a refresh.
function UIManager:widgetInvert(widget, x, y, w, h)
    if not widget then
        return
    end
    Screen.bb:invertRect(x, y, w or widget.dimen.w, h or widget.dimen.h)
end

function UIManager:waitForVSync() end

function UIManager:yieldToEPDC() end

function UIManager:avoidFlashOnNextRepaint() end

function UIManager:setRefreshRate(rate, night_rate)
    if rate then
        self.FULL_REFRESH_COUNT = rate
    end
    if night_rate and G_reader_settings and G_reader_settings:isTrue("night_mode") then
        self.FULL_REFRESH_COUNT = night_rate
    end
end

function UIManager:getRefreshRate()
    return self.FULL_REFRESH_COUNT, self.FULL_REFRESH_COUNT
end

-- The Kaia app owns the main loop, so a plugin cannot quit it. The call is
-- logged and returns the exit code it was given.
function UIManager:suspend()
    if KAIA_DEVICE then KAIA_DEVICE("suspend", true) end
end

function UIManager:quit(exit_code)
    logger.info("UIManager:quit ignored by the Kaia Lua host", exit_code)
    return exit_code
end

-- KOReader shows a dialog that asks the user to restart. The Kaia host has no
-- restart, so the message is shown and the user restarts by hand.
function UIManager:askForRestart(message_text)
    local InfoMessage = require("ui/widget/infomessage")
    self:show(InfoMessage:new{
        text = message_text or _("Restart needed to apply the changes."),
    })
end

function UIManager:flushSettings()
    if KAIA_READER then KAIA_READER.flush_settings() end
    if KAIA_PLUGIN then KAIA_PLUGIN.dispatch("FlushSettings") end
    if G_reader_settings then
        G_reader_settings:flush()
    end
end

return UIManager
