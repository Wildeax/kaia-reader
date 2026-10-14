--[[--
Kaia online lookups: the Translate and Wikipedia actions.

Both actions send the selected text over the network. Each one has a switch
in G_reader_settings, and both are off by default. Turning one on asks for
confirmation first. Turning one off takes effect at once.

Upstream code asks isAllowed() before it adds a button or a long-press
choice, and before it makes a request. menuItem() builds the checkbox for the
Kaia settings in the file manager menu.

@module kaia.online_lookups
]]

local ConfirmBox = require("ui/widget/confirmbox")
local UIManager = require("ui/uimanager")
local _ = require("gettext")

local OnlineLookups = {}

-- One switch per online action. The keys are the action names the reader uses.
OnlineLookups.settings = {
    translate = "kaia_allow_translate",
    wikipedia = "kaia_allow_wikipedia",
}

local function menuText(kind)
    if kind == "translate" then
        return _("Translate selected text")
    end
    return _("Wikipedia lookup for selected text")
end

local function helpText(kind)
    if kind == "translate" then
        return _("Off by default. When on, selected text is sent to Google Translate over the internet.")
    end
    return _("Off by default. When on, selected text is sent to Wikipedia over the internet.")
end

local function confirmText(kind)
    if kind == "translate" then
        return _("Selected text will be sent to Google Translate over the internet.")
    end
    return _("Selected text will be sent to Wikipedia over the internet.")
end

-- True only when the switch is on. A missing setting counts as off.
function OnlineLookups:isAllowed(kind)
    local setting = self.settings[kind]
    return setting ~= nil and G_reader_settings:isTrue(setting)
end

-- For long-press actions and their menus: an online action is allowed only
-- while its switch is on. Every other action is always allowed.
function OnlineLookups:allowsAction(action)
    if self.settings[action] then
        return self:isAllowed(action)
    end
    return true
end

-- Turns a switch off at once. Turning it on shows the disclosure first.
function OnlineLookups:toggle(kind)
    local setting = self.settings[kind]
    if self:isAllowed(kind) then
        G_reader_settings:saveSetting(setting, false)
        return
    end
    UIManager:show(ConfirmBox:new{
        text = confirmText(kind),
        ok_text = _("Turn on"),
        cancel_text = _("Cancel"),
        ok_callback = function()
            G_reader_settings:saveSetting(setting, true)
        end,
    })
end

-- Checkbox menu entry for one switch.
function OnlineLookups:menuItem(kind)
    return {
        text = menuText(kind),
        help_text = helpText(kind),
        checked_func = function()
            return self:isAllowed(kind)
        end,
        callback = function()
            self:toggle(kind)
        end,
    }
end

return OnlineLookups
