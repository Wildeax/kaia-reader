--[[--
Kaia extension API, version 1.

A small declarative layer over KOReader's dispatcher, main menu, settings and
dialogs. A plugin describes the commands, menu items and settings it adds.
Kaia builds the KOReader tables for them, so the menus and dialogs look the
same across plugins.

This is a convenience layer, not a sandbox. A plugin that uses it runs as
trusted code with full access to Lua, FFI and the file system. See
frontend/kaia/api/README.md and docs/kaia/ENGINEERING_PLAN.md section 5.

Usage:
    local KaiaAPI = require("kaia/api/v1")
    local api, reason = KaiaAPI.require(1)
    if not api then
        error(reason)
    end

Every registration function takes `plugin = self`, the plugin instance. The
instance supplies the plugin name (used in keys) and `ui` (passed to `run`).

@module kaia.api.v1
]]

local ConfirmBox = require("ui/widget/confirmbox")
local Dispatcher = require("dispatcher")
local InfoMessage = require("ui/widget/infomessage")
local SpinWidget = require("ui/widget/spinwidget")
local UIManager = require("ui/uimanager")
local logger = require("logger")

local API = {
    version = 1,
}

-- Section names used by addMenuItem and addSetting, mapped to the keys of
-- the KOReader menu order files (frontend/ui/elements/*_menu_order.lua).
local SECTIONS = {
    tools = "tools",
    more_tools = "more_tools",
    settings = "setting",
}
local DEFAULT_MENU_SECTION = "more_tools"
local DEFAULT_SETTING_SECTION = "settings"

local SETTING_KINDS = {
    toggle = true,
    choice = true,
    number = true,
}

-- Identifiers for command, menu item and setting ids: letters, digits and underscores.
local ID_PATTERN = "^[%a][%w_]*$"
-- Event names become handler names ("on" .. event), so they must be identifiers.
local EVENT_PATTERN = "^[%a_][%w_]*$"

-- Menu items and commands registered through this API, per plugin instance.
-- A weak key lets a closed plugin instance be collected.
local registries = setmetatable({}, { __mode = "k" })

-- Raises an error that names the field and the function.
local function argError(fn, field, reason)
    error(string.format("KaiaAPI.%s: %s %s", fn, field, reason), 3)
end

local function requireTable(fn, opts)
    if type(opts) ~= "table" then
        error(string.format("KaiaAPI.%s: expected a table of options, got %s", fn, type(opts)), 3)
    end
    return opts
end

local function requireString(fn, opts, field)
    local value = opts[field]
    if type(value) ~= "string" or value == "" then
        argError(fn, "'" .. field .. "'", "must be a non-empty string")
    end
    return value
end

local function optionalString(fn, opts, field)
    if opts[field] == nil then
        return nil
    end
    return requireString(fn, opts, field)
end

local function requireFunction(fn, opts, field)
    local value = opts[field]
    if type(value) ~= "function" then
        argError(fn, "'" .. field .. "'", "must be a function")
    end
    return value
end

local function requireId(fn, opts)
    local id = requireString(fn, opts, "id")
    if not id:match(ID_PATTERN) then
        argError(fn, "'id'", "must start with a letter and use only letters, digits and underscores")
    end
    return id
end

local function requirePlugin(fn, opts)
    local plugin = opts.plugin
    if type(plugin) ~= "table" or type(plugin.name) ~= "string" or plugin.name == "" then
        argError(fn, "'plugin'", "must be the plugin instance, a table with a name (pass plugin = self)")
    end
    return plugin
end

local function requireSection(fn, opts, default)
    local section = opts.section or default
    if SECTIONS[section] == nil then
        argError(fn, "'section'", "must be one of 'tools', 'settings' or 'more_tools'")
    end
    return section
end

-- Returns the registry for a plugin instance, and checks for duplicate ids.
local function registryFor(fn, plugin, kind, id)
    local registry = registries[plugin]
    if not registry then
        registry = { seen = {}, menu_items = {} }
        registries[plugin] = registry
    end
    local key = kind .. ":" .. id
    if registry.seen[key] then
        argError(fn, "'id'", string.format("'%s' is already registered by this plugin", id))
    end
    registry.seen[key] = true
    return registry
end

-- Adds the plugin's menu items to the main menu. The menu asks for the items on
-- each rebuild, so the item tables are created fresh each time (the menu sorter
-- modifies them).
local function attachMenu(plugin, registry)
    if registry.attached then
        return
    end
    local menu = plugin.ui and plugin.ui.menu
    if not (menu and menu.registerToMainMenu) then
        error("KaiaAPI: the plugin has no main menu (plugin.ui.menu) to attach items to", 3)
    end
    local bridge = {}
    function bridge:addToMainMenu(menu_items)
        for key, build in pairs(registry.menu_items) do
            menu_items[key] = build()
        end
    end
    menu:registerToMainMenu(bridge)
    registry.attached = true
end

local function menuKey(plugin, id)
    return "kaia_ext_" .. plugin.name .. "_" .. id
end

local function settingKey(plugin, id)
    return "kaia_ext_" .. plugin.name .. "_" .. id
end

local function eventName(plugin, id)
    return "KaiaApi_" .. plugin.name .. "_" .. id
end

local function actionName(plugin, id)
    return "kaia_ext_" .. plugin.name .. "_" .. id
end

--- Registers a command that appears in the Dispatcher, so users can bind it to a
-- gesture or a quick menu. `run` is called with the plugin's `ui`.
-- @tparam table opts `plugin`, `id`, `title`, `run`, and optional `event_name`
-- @treturn table handle with `id`, `action` (the Dispatcher action name), `event` and `title`
function API.addCommand(opts)
    local fn = "addCommand"
    opts = requireTable(fn, opts)
    local plugin = requirePlugin(fn, opts)
    local id = requireId(fn, opts)
    local title = requireString(fn, opts, "title")
    local run = requireFunction(fn, opts, "run")
    local event = optionalString(fn, opts, "event_name") or eventName(plugin, id)
    if not event:match(EVENT_PATTERN) then
        argError(fn, "'event_name'", "must be an identifier (letters, digits and underscores)")
    end
    registryFor(fn, plugin, "command", id)

    local action = actionName(plugin, id)
    Dispatcher:registerAction(action, {
        category = "none",
        event = event,
        title = title,
        general = true,
    })
    -- Dispatcher events reach plugin instances through their "on<event>" method.
    plugin["on" .. event] = function()
        local ok, err = pcall(run, plugin.ui)
        if not ok then
            logger.err("KaiaAPI command failed:", plugin.name, id, err)
        end
        return true
    end
    return { id = id, action = action, event = event, title = title }
end

-- Builds the sub-item table for addMenuItem's `sub_items`.
local function buildSubItems(fn, plugin, sub_items)
    if type(sub_items) ~= "table" or #sub_items == 0 then
        argError(fn, "'sub_items'", "must be a non-empty array of { title, run } tables")
    end
    local out = {}
    for i, sub in ipairs(sub_items) do
        if type(sub) ~= "table" or type(sub.title) ~= "string" or sub.title == "" then
            argError(fn, string.format("'sub_items[%d].title'", i), "must be a non-empty string")
        end
        if type(sub.run) ~= "function" then
            argError(fn, string.format("'sub_items[%d].run'", i), "must be a function")
        end
        local title, run = sub.title, sub.run
        out[i] = {
            text = title,
            callback = function()
                local ok, err = pcall(run, plugin.ui)
                if not ok then
                    logger.err("KaiaAPI menu item failed:", plugin.name, title, err)
                end
            end,
        }
    end
    return out
end

--- Adds a main menu item for the plugin. Give either `run` or `sub_items`.
-- @tparam table opts `plugin`, `id`, `title`, `section` ("tools", "settings" or
-- "more_tools", default "more_tools"), `run` or `sub_items` (array of
-- `{ title, run }`), and optional `enabled_func`
-- @treturn table handle with `id` and `key` (the menu key)
function API.addMenuItem(opts)
    local fn = "addMenuItem"
    opts = requireTable(fn, opts)
    local plugin = requirePlugin(fn, opts)
    local id = requireId(fn, opts)
    local title = requireString(fn, opts, "title")
    local section = requireSection(fn, opts, DEFAULT_MENU_SECTION)
    if (opts.run == nil) == (opts.sub_items == nil) then
        argError(fn, "'run' or 'sub_items'", "must give exactly one of them")
    end
    local run = opts.run ~= nil and requireFunction(fn, opts, "run") or nil
    if opts.enabled_func ~= nil then
        requireFunction(fn, opts, "enabled_func")
    end
    local sub_items = opts.sub_items ~= nil and buildSubItems(fn, plugin, opts.sub_items) or nil
    local registry = registryFor(fn, plugin, "menu", id)
    local enabled_func = opts.enabled_func

    local key = menuKey(plugin, id)
    registry.menu_items[key] = function()
        local item = {
            text = title,
            sorting_hint = SECTIONS[section],
            enabled_func = enabled_func,
        }
        if run then
            item.callback = function()
                local ok, err = pcall(run, plugin.ui)
                if not ok then
                    logger.err("KaiaAPI menu item failed:", plugin.name, id, err)
                end
            end
        else
            -- Fresh sub-item tables for each build, since the menu sorter edits them.
            item.sub_item_table = {}
            for i, sub in ipairs(sub_items) do
                item.sub_item_table[i] = { text = sub.text, callback = sub.callback }
            end
        end
        return item
    end
    attachMenu(plugin, registry)
    return { id = id, key = key }
end

-- Checks a value against a setting definition. Returns true or nil.
local function settingAccepts(def, value)
    if def.kind == "toggle" then
        return type(value) == "boolean"
    elseif def.kind == "number" then
        return type(value) == "number" and value >= def.min and value <= def.max
    end
    return def.values[value] == true
end

-- Validates the kind-specific fields of addSetting and returns a definition.
local function settingDefinition(fn, opts)
    local kind = opts.kind
    if not SETTING_KINDS[kind] then
        argError(fn, "'kind'", "must be 'toggle', 'choice' or 'number'")
    end
    if opts.default == nil then
        argError(fn, "'default'", "is required")
    end
    local def = { kind = kind, default = opts.default }
    if kind == "toggle" then
        if type(opts.default) ~= "boolean" then
            argError(fn, "'default'", "must be a boolean for kind 'toggle'")
        end
    elseif kind == "number" then
        if type(opts.default) ~= "number" then
            argError(fn, "'default'", "must be a number for kind 'number'")
        end
        if type(opts.min) ~= "number" then
            argError(fn, "'min'", "must be a number for kind 'number'")
        end
        if type(opts.max) ~= "number" then
            argError(fn, "'max'", "must be a number for kind 'number'")
        end
        if opts.min > opts.max then
            argError(fn, "'min'", "must not be greater than 'max'")
        end
        if opts.default < opts.min or opts.default > opts.max then
            argError(fn, "'default'", "must be between 'min' and 'max'")
        end
        def.min, def.max = opts.min, opts.max
    else
        if type(opts.choices) ~= "table" or #opts.choices == 0 then
            argError(fn, "'choices'", "must be a non-empty array for kind 'choice'")
        end
        def.choices = {}
        def.values = {}
        for i, choice in ipairs(opts.choices) do
            local where = string.format("'choices[%d]'", i)
            local value, text
            if type(choice) == "string" and choice ~= "" then
                value, text = choice, choice
            elseif type(choice) == "table" and type(choice.text) == "string" and choice.text ~= ""
                    and (type(choice.value) == "string" or type(choice.value) == "number") then
                value, text = choice.value, choice.text
            else
                argError(fn, where, "must be a non-empty string or a table with 'value' and 'text'")
            end
            if def.values[value] then
                argError(fn, where, "repeats a value already in 'choices'")
            end
            def.choices[i] = { value = value, text = text }
            def.values[value] = true
        end
        if not def.values[opts.default] then
            argError(fn, "'default'", "must be one of the 'choices' values")
        end
    end
    return def
end

--- Adds a setting and its menu item. The value is stored in G_reader_settings under
-- `kaia_ext_<plugin>_<id>`. Settings are read and written through the returned
-- getter and setter, which return the default while nothing is stored.
-- @tparam table opts `plugin`, `id`, `title`, `kind` ("toggle", "choice" or "number"),
-- `default`, `choices` (for "choice": strings, or `{ value, text }` tables),
-- `min` and `max` (both required for "number"), and optional `section`
-- ("tools", "settings" or "more_tools", default "settings")
-- @treturn function getter returns the current value
-- @treturn function setter stores a value of the right kind, or raises an error
function API.addSetting(opts)
    local fn = "addSetting"
    opts = requireTable(fn, opts)
    local plugin = requirePlugin(fn, opts)
    local id = requireId(fn, opts)
    local title = requireString(fn, opts, "title")
    local section = requireSection(fn, opts, DEFAULT_SETTING_SECTION)
    local def = settingDefinition(fn, opts)
    local registry = registryFor(fn, plugin, "setting", id)
    local key = settingKey(plugin, id)

    local function get()
        local value = G_reader_settings:readSetting(key)
        if value ~= nil and settingAccepts(def, value) then
            return value
        end
        return def.default
    end

    local function set(value)
        if not settingAccepts(def, value) then
            argError("setting " .. id, "value", "is not valid for this setting: " .. tostring(value))
        end
        G_reader_settings:saveSetting(key, value)
    end

    local menu_key = "kaia_ext_" .. plugin.name .. "_setting_" .. id
    registry.menu_items[menu_key] = function()
        local item = {
            text = title,
            sorting_hint = SECTIONS[section],
        }
        if def.kind == "toggle" then
            item.checked_func = get
            item.keep_menu_open = true
            item.callback = function()
                set(not get())
            end
        elseif def.kind == "choice" then
            item.text_func = function()
                for _i, choice in ipairs(def.choices) do
                    if choice.value == get() then
                        return title .. ": " .. choice.text
                    end
                end
                return title
            end
            item.sub_item_table = {}
            for i, choice in ipairs(def.choices) do
                item.sub_item_table[i] = {
                    text = choice.text,
                    checked_func = function() return get() == choice.value end,
                    keep_menu_open = true,
                    callback = function() set(choice.value) end,
                }
            end
        else
            item.text_func = function()
                return title .. ": " .. tostring(get())
            end
            item.keep_menu_open = true
            item.callback = function(touchmenu_instance)
                UIManager:show(SpinWidget:new{
                    title_text = title,
                    value = get(),
                    value_min = def.min,
                    value_max = def.max,
                    default_value = def.default,
                    callback = function(spin)
                        set(spin.value)
                        if touchmenu_instance then
                            touchmenu_instance:updateItems()
                        end
                    end,
                })
            end
        end
        return item
    end
    attachMenu(plugin, registry)
    return get, set
end

--- Shows a message box. `kind` is "info" (default) or "warning".
-- @treturn InfoMessage the widget shown
function API.showMessage(opts)
    local fn = "showMessage"
    opts = requireTable(fn, opts)
    local text = requireString(fn, opts, "text")
    local kind = opts.kind or "info"
    if kind ~= "info" and kind ~= "warning" then
        argError(fn, "'kind'", "must be 'info' or 'warning'")
    end
    local widget = InfoMessage:new{
        text = text,
        icon = kind == "warning" and "notice-warning" or "notice-info",
    }
    UIManager:show(widget)
    return widget
end

--- Asks the user to confirm. `on_ok` runs only when the user taps the OK button.
-- @treturn ConfirmBox the widget shown
function API.confirm(opts)
    local fn = "confirm"
    opts = requireTable(fn, opts)
    local text = requireString(fn, opts, "text")
    local on_ok = requireFunction(fn, opts, "on_ok")
    local ok_text = optionalString(fn, opts, "ok_text")
    local cancel_text = optionalString(fn, opts, "cancel_text")
    local widget = ConfirmBox:new{
        text = text,
        ok_text = ok_text,
        cancel_text = cancel_text,
        ok_callback = on_ok,
    }
    UIManager:show(widget)
    return widget
end

--- Returns the API table for a major version, or nil and a reason when the
-- major version is not available in this build.
function API.require(major)
    if major == API.version then
        return API
    end
    if type(major) ~= "number" then
        return nil, "KaiaAPI.require: the major version must be a number"
    end
    return nil, string.format("Kaia API major version %s is not available. This build provides version %d.",
        tostring(major), API.version)
end

return API
