local Dispatcher = { settingsList = {} }
local _ = require("gettext")
function Dispatcher:init()
    self:registerAction("font_size", {category="absolutenumber", event="SetFontSize", title=_("Font size"), rolling=true})
    self:registerAction("set_font", {category="string", event="SetFont", title=_("Font"), rolling=true})
end
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
function Dispatcher:execute(settings, props)
    if type(settings) == "string" then settings = { [settings] = props or true }; props = nil end
    for id in pairs(settings) do
        if id ~= "settings" and not self.settingsList[id] then
            error("dispatcher action " .. tostring(id) .. " is unavailable", 2)
        end
    end
    local events, meta = KAIA_DISPATCH(settings, props or {})
    settings.settings = meta
    local Event = require("ui/event")
    local UIManager = require("ui/uimanager")
    for _, event in ipairs(events) do
        if event.broadcast then UIManager:broadcastEvent(Event:new(event.name, unpack(event.args, 1, event.n)))
        else UIManager:sendEvent(Event:new(event.name, unpack(event.args, 1, event.n))) end
    end
end
function Dispatcher:_itemsCount(settings)
    local count = 0
    for id in pairs(settings or {}) do if id ~= "settings" then count = count + 1 end end
    return count
end
function Dispatcher:getNameFromItem(id)
    return self.settingsList[id] and self.settingsList[id].title or id
end
function Dispatcher:menuTextFunc(settings)
    if settings == nil then return _("Pass through") end
    local names = {}
    for id in pairs(settings) do if id ~= "settings" then names[#names + 1] = self:getNameFromItem(id) end end
    table.sort(names)
    return #names == 0 and _("Nothing") or table.concat(names, ", ")
end
function Dispatcher.removeActions(_, callback) callback() end
function Dispatcher:getArgFromValue(_, value) return value end
function Dispatcher:addSubMenu(caller, menu, location, key)
    local ids = {}
    for id in pairs(self.settingsList) do ids[#ids + 1] = id end
    table.sort(ids)
    for _, id in ipairs(ids) do
        local action = self.settingsList[id]
        -- Actions that require a value need the host's setting editor.
        if action.category == "none" then
            menu[#menu + 1] = {
                text = action.title,
                checked_func = function() return location[key] and location[key][id] ~= nil end,
                callback = function()
                    location[key] = location[key] or {}
                    location[key][id] = not location[key][id] or nil
                    caller.updated = true
                end,
            }
        end
    end
end
return Dispatcher
