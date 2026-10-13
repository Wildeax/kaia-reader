-- KOReader plugin entries use TouchMenu callbacks inside a standard Menu.
local Menu = require("ui/widget/menu")
local UIManager = require("ui/uimanager")
local PluginMenu = Menu:extend{}
function PluginMenu:onMenuSelect(item)
    if item.enabled == false or (item.enabled_func and not item.enabled_func()) then return true end
    if item.sub_item_table_func then item.sub_item_table = item.sub_item_table_func() end
    if item.sub_item_table then return Menu.onMenuSelect(self, item) end
    if item.tap_input_func then
        local spec = item.tap_input_func()
        if not spec then return true end
        local dialog
        dialog = require("ui/widget/inputdialog"):new{
            title = spec.title, input = spec.input or "", input_type = spec.input_type,
            buttons = {{
                { text = require("gettext")("Cancel"), id = "close", callback = function() UIManager:close(dialog) end },
                { text = require("gettext")("Save"), callback = function()
                    spec.callback(dialog:getInputText())
                    UIManager:close(dialog)
                    self:updateItems()
                end },
            }},
        }
        UIManager:show(dialog)
        dialog:onShowKeyboard()
    elseif item.callback then
        item.callback(self)
        self:updateItems()
    end
    return true
end
function PluginMenu:closeMenu() UIManager:close(self) end
function PluginMenu:onMenuHold(item)
    if item.hold_callback then item.hold_callback(self, item) end
    return true
end
return PluginMenu
