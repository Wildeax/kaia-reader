local Dispatcher = { settingsList = {} }
function Dispatcher:registerAction(id, action)
    if self.settingsList[id] then return false end
    self.settingsList[id] = action
    KAIA_PLUGIN.actions[id] = action
    return true
end
function Dispatcher:removeAction(id)
    self.settingsList[id] = nil
    KAIA_PLUGIN.actions[id] = nil
end
function Dispatcher:execute(id, value)
    local item = self.settingsList[id]
    if not item then error("dispatcher action " .. tostring(id) .. " is unavailable", 2) end
    local Event = require("ui/event")
    local UIManager = require("ui/uimanager")
    UIManager:sendEvent(Event:new(item.event, value or item.arg))
end
return Dispatcher
