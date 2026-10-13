--[[--
Kaia example: a plugin that uses each function of the Kaia extension API, version 1.

The plugin is disabled by default, like hello.koplugin. To try it, set
DISABLED_BY_DEFAULT to false and restart Kaia. The API is described in frontend/kaia/api/README.md.

@module koplugin.KaiaExample
--]]--

-- Disabled by default, as hello.koplugin is. Set this to false to load the plugin.
local DISABLED_BY_DEFAULT = true
if DISABLED_BY_DEFAULT then
    return { disabled = true, }
end

local KaiaAPI = require("kaia/api/v1")
local WidgetContainer = require("ui/widget/container/widgetcontainer")
local logger = require("logger")
local _ = require("gettext")

local api, reason = KaiaAPI.require(1)
if not api then
    logger.warn("kaiaexample:", reason)
    return { disabled = true, }
end

local KaiaExample = WidgetContainer:extend{
    name = "kaiaexample",
    is_doc_only = false,
}

function KaiaExample:init()
    -- A command. It can be bound to a gesture in the Dispatcher.
    self.command = api.addCommand{
        plugin = self,
        id = "say_hello",
        title = _("Kaia example: say hello"),
        run = function()
            api.showMessage{ text = _("Hello from the Kaia example plugin.") }
        end,
    }

    -- A toggle setting. The getter and setter store the value in G_reader_settings.
    self.get_shout, self.set_shout = api.addSetting{
        plugin = self,
        id = "shout",
        title = _("Shout the greeting"),
        kind = "toggle",
        default = false,
    }

    -- A menu item in the Tools tab. It asks first, then shows the greeting.
    api.addMenuItem{
        plugin = self,
        id = "greet",
        title = _("Kaia example greeting"),
        section = "tools",
        run = function()
            api.confirm{
                text = _("Show the greeting now?"),
                ok_text = _("Show"),
                on_ok = function()
                    local text = _("Hello from the Kaia example plugin.")
                    if self.get_shout() then
                        text = text:upper()
                    end
                    api.showMessage{ text = text, kind = "info" }
                end,
            }
        end,
    }
end

return KaiaExample
