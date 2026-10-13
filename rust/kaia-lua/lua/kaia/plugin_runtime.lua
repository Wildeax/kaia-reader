local M = { menus = {}, menu_owners = {}, actions = {}, instances = {}, status = {}, view_modules = {}, shortcuts = {}, highlight_dialog = {} }
KAIA_PLUGIN = M

local function failed(name, message)
    M.status[name] = { loaded = false, error = tostring(message) }
    require("logger").err("Plugin failed", name, tostring(message))
    M.instances[name] = nil
    for id, owner in pairs(M.menu_owners) do
        if owner == name then M.menus[id] = nil end
    end
end

local function guarded(name, fn, ...)
    if M.status[name] and not M.status[name].loaded then return false end
    local ok, result = pcall(fn, ...)
    if not ok then failed(name, result) end
    return ok, result
end

function M.wrap_menu(name, item)
    for _, field in ipairs({ "callback", "hold_callback", "enabled_func", "checked_func", "text_func" }) do
        local fn = item[field]
        if type(fn) == "function" then
            item[field] = function(...)
                local ok, result = guarded(name, fn, ...)
                if ok then return result end
            end
        end
    end
    if type(item.sub_item_table) == "table" then
        for _, child in ipairs(item.sub_item_table) do M.wrap_menu(name, child) end
    end
end

function M.load(paths, names, safe_mode, force_hello, reader_mode)
    M.menus, M.menu_owners, M.actions, M.instances, M.status = {}, {}, {}, {}, {}
    M.view_modules, M.shortcuts, M.highlight_dialog = {}, {}, {}
    local Loader = require("pluginloader")
    local original_discover = Loader._discover
    local original_dofile = dofile
    local wanted = {}
    for _, name in ipairs(names) do wanted[name] = true end
    Loader._discover = function()
        local found = {}
        local lfs = require("libs/libkoreader-lfs")
        for path_index, root in ipairs(paths) do
            if path_index == 1 or not safe_mode then
                for entry in lfs.dir(root) do
                    local name = entry:match("^(.*)%.koplugin$")
                    if name and (#names == 0 or wanted[name]) then
                        local path = root .. "/" .. entry
                        if lfs.attributes(path, "mode") == "directory" then
                            found[#found + 1] = {
                                name = name, path = path,
                                main = path .. "/main.lua", meta = path .. "/_meta.lua",
                                disabled = false,
                            }
                        end
                    end
                end
            end
        end
        return found
    end
    if force_hello then
        dofile = function(path)
            if path:match("/hello%.koplugin/main%.lua$") then
                local file = assert(io.open(path, "r"))
                local source = file:read("*a")
                file:close()
                source = source:gsub("if true then%s*return { disabled = true, }%s*end", "")
                return assert(loadstring(source, "@" .. path))()
            end
            return original_dofile(path)
        end
    end
    Loader.enabled_plugins = nil
    Loader.disabled_plugins = nil
    Loader.load_errors = nil
    local ok, modules = pcall(function() return Loader:loadPlugins() end)
    dofile = original_dofile
    Loader._discover = original_discover
    if not ok then error(modules, 0) end
    for name, message in pairs(Loader.load_errors or {}) do failed(name, message) end
    local ui = require(reader_mode and "apps/reader/readerui" or "apps/filemanager/filemanager")
    for _, module in ipairs(modules) do
        if reader_mode or not module.is_doc_only then
            local name = module.name
            local attr = { ui = ui }
            if reader_mode then
                attr.document = ui.document
                attr.view = ui.view
            end
            local instance_ok, instance = Loader:createPluginInstance(module, attr)
            if instance_ok and instance then
                M.instances[name] = instance
                M.status[name] = { loaded = true }
            else
                failed(name, instance or "plugin did not create an instance")
            end
        end
    end
end

function M.dispatch(name, ...)
    local method = "on" .. name
    for plugin_name, instance in pairs(M.instances) do
        local handler = instance[method]
        if handler then
            if type(handler) == "table" and handler.f then handler = handler.f end
            if name == "ReaderReady" and select("#", ...) == 0 then
                guarded(plugin_name, handler, instance, instance.ui.doc_settings)
            else
                guarded(plugin_name, handler, instance, ...)
            end
        end
    end
end

function M.open_menu(id)
    local item = M.menus[id]
    if not item then error("main menu entry " .. tostring(id) .. " is unavailable", 2) end
    local name = M.menu_owners[id]
    if item.sub_item_table then
        local Menu = require("ui/widget/menu")
        local UIManager = require("ui/uimanager")
        guarded(name, function() UIManager:show(Menu:new{ item_table = item.sub_item_table, title = item.text }) end)
    elseif item.callback then
        guarded(name, item.callback)
    else
        failed(name, "main menu entry " .. id .. " has no action")
    end
end

return M
