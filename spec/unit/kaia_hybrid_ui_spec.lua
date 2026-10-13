-- KR-010: Kaia Home and an unchanged plugin dialog share one window stack and
-- one UIManager run loop. See docs/kaia/specs/KR-010-hybrid-ui.md.

describe("kaia hybrid UI", function()
    local Blitbuffer, ConfirmBox, Device, Event, Geom, Home, Screen, UIManager
    local home, dialog, original_rotation

    setup(function()
        require("commonrequire")
        disable_plugins()
        require("document/canvascontext"):init(require("device"))
        Blitbuffer = require("ffi/blitbuffer")
        ConfirmBox = require("ui/widget/confirmbox")
        Device = require("device")
        Event = require("ui/event")
        Geom = require("ui/geometry")
        Home = require("kaia/home")
        Screen = Device.screen
        UIManager = require("ui/uimanager")
    end)

    before_each(function()
        home, dialog = nil, nil
        original_rotation = Screen:getRotationMode()
    end)

    after_each(function()
        if dialog and UIManager:isWidgetShown(dialog) then
            UIManager:close(dialog)
        end
        Home.closeIfShown()
        Screen:setRotationMode(original_rotation)
    end)

    -- Paints a widget tree once, so that every child gets its position.
    local function paint(widget)
        local bb = Blitbuffer.new(Screen:getWidth(), Screen:getHeight(), Blitbuffer.TYPE_BB8)
        widget:paintTo(bb, 0, 0)
        bb:free()
    end

    -- First button with `text` under `widget`, depth first, or nil.
    local function findButton(widget, text, depth)
        depth = depth or 0
        if type(widget) ~= "table" or depth > 12 then
            return nil
        end
        if widget.text == text and type(widget.callback) == "function" then
            return widget
        end
        for _, child in ipairs(widget) do
            local found = findButton(child, text, depth + 1)
            if found then
                return found
            end
        end
        return nil
    end

    -- Center of a painted button, as a zero-size tap position.
    local function centerOf(button)
        local dimen = button[1].dimen
        return Geom:new{ x = dimen.x + dimen.w / 2, y = dimen.y + dimen.h / 2, w = 0, h = 0 }
    end

    -- Runs `fn` and returns how many timer or task calls it made.
    local function countScheduling(fn)
        local calls = 0
        local names = { "schedule", "scheduleIn", "nextTick", "tickAfterNext" }
        local originals = {}
        for _, name in ipairs(names) do
            originals[name] = UIManager[name]
            UIManager[name] = function(self, ...)
                calls = calls + 1
                return originals[name](self, ...)
            end
        end
        local ok, err = pcall(fn)
        for _, name in ipairs(names) do
            UIManager[name] = originals[name]
        end
        assert(ok, err)
        return calls
    end

    -- A ConfirmBox created the way plugins create one.
    local function showPluginDialog(on_cancel)
        dialog = ConfirmBox:new{
            text = "Remove this book from the library?",
            ok_text = "Remove",
            cancel_text = "Cancel",
            ok_callback = function() end,
            cancel_callback = on_cancel,
        }
        UIManager:show(dialog)
        return dialog
    end

    it("shows Home without registering a timer or task", function()
        local queue_before = #UIManager._task_queue
        local calls = countScheduling(function()
            home = Home:show()
        end)
        assert.are.equal(0, calls)
        assert.are.equal(queue_before, #UIManager._task_queue)
        assert.are.equal(home, UIManager:getTopmostVisibleWidget())
    end)

    it("puts a plugin dialog on top of Home and sends input to it", function()
        home = Home:show()
        local cancelled = 0
        showPluginDialog(function() cancelled = cancelled + 1 end)

        assert.are.equal(dialog, UIManager:getTopmostVisibleWidget())
        assert.is_true(UIManager:isWidgetShown(home))

        paint(dialog)
        local cancel = findButton(dialog, "Cancel")
        assert.is_not_nil(cancel, "the dialog has a Cancel button")
        UIManager:sendEvent(Event:new("Gesture", { ges = "tap", pos = centerOf(cancel) }))

        assert.are.equal(1, cancelled)
        assert.is_false(UIManager:isWidgetShown(dialog))
    end)

    it("returns the top of the stack to Home after the dialog closes", function()
        home = Home:show()
        showPluginDialog()
        paint(dialog)

        UIManager:close(dialog)
        assert.are.equal(home, UIManager:getTopmostVisibleWidget())
        assert.is_true(UIManager:isWidgetShown(home))
    end)

    it("repaints the stack without error after a rotation", function()
        home = Home:show()
        showPluginDialog()
        paint(dialog)

        Screen:setRotationMode(Screen.DEVICE_ROTATED_CLOCKWISE)
        UIManager:broadcastEvent(Event:new("SetDimensions", Screen:getSize()))
        UIManager:onRotation()

        assert.has_no.errors(function()
            paint(home)
            paint(dialog)
        end)
        assert.are.equal(dialog, UIManager:getTopmostVisibleWidget())
        assert.is_true(UIManager:isWidgetShown(home))
    end)

    it("keeps Home and the dialog through a suspend and resume pair", function()
        home = Home:show()
        showPluginDialog()
        paint(dialog)

        UIManager:broadcastEvent(Event:new("Suspend"))
        UIManager:broadcastEvent(Event:new("Resume"))

        assert.is_true(UIManager:isWidgetShown(home))
        assert.is_true(UIManager:isWidgetShown(dialog))
        assert.are.equal(dialog, UIManager:getTopmostVisibleWidget())
        assert.has_no.errors(function()
            paint(dialog)
        end)
    end)
end)
