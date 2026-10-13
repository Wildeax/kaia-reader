local M = { menus = {}, menu_owners = {}, actions = {}, instances = {}, status = {}, view_modules = {}, shortcuts = {}, highlight_dialog = {} }
KAIA_PLUGIN = M

local function failed(name, message)
    name = name or "unknown"
    M.status[name] = { loaded = false, error = tostring(message) }
    require("logger").err("Plugin failed", name, tostring(message))
    M.instances[name] = nil
    if M.ui then M.ui[name] = nil end
    for id, module in pairs(M.view_modules) do
        if module.name == name then M.view_modules[id] = nil end
    end
    for id, owner in pairs(M.menu_owners) do
        if owner == name then M.menus[id] = nil end
    end
end

M.failed = failed

local function guarded(name, fn, ...)
    if M.loading_name ~= name and (not M.status[name] or not M.status[name].loaded) then return false end
    local previous = M.owner
    M.owner = name
    local results = table.pack(pcall(fn, ...))
    M.owner = previous
    if not results[1] then failed(name, results[2]) end
    return unpack(results, 1, results.n)
end

M.guarded = guarded

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
    if item.tap_input_func then
        local fn = item.tap_input_func
        item.tap_input_func = function(...)
            local ok, spec = guarded(name, fn, ...)
            if not ok or not spec then return end
            if spec.callback then
                local callback = spec.callback
                spec.callback = function(...) return guarded(name, callback, ...) end
            end
            return spec
        end
    end
    if item.sub_item_table_func then
        local fn = item.sub_item_table_func
        item.sub_item_table_func = function(...)
            local ok, children = guarded(name, fn, ...)
            if not ok then return {} end
            for _, child in ipairs(children) do M.wrap_menu(name, child) end
            return children
        end
    end
    if type(item.sub_item_table) == "table" then
        for _, child in ipairs(item.sub_item_table) do M.wrap_menu(name, child) end
    end
end

function M.load(paths, names, safe_mode, force_hello, reader_mode)
    M.generation = (M.generation or 0) + 1
    for name, instance in pairs(M.instances) do
        if instance.onCloseWidget then guarded(name, instance.onCloseWidget, instance) end
        if M.ui then M.ui[name] = nil end
    end
    M.touch_zones = {}
    require("pluginshare").pause_auto_suspend = false
    require("ui/uimanager")._standby_holds = 0
    local footer = package.loaded["kaia/reader_footer"]
    if footer then footer.content = {} end
    local dispatcher = package.loaded.dispatcher
    if dispatcher then dispatcher.settingsList = {} end
    M.menus, M.menu_owners, M.actions, M.instances, M.status = {}, {}, {}, {}, {}
    M.view_modules, M.shortcuts, M.highlight_dialog = {}, {}, {}
    local Loader = require("pluginloader")
    local original_discover = Loader._discover
    local original_dofile = dofile
    local wanted, discovered = {}, {}
    for _, name in ipairs(names) do wanted[name] = true end
    Loader._discover = function()
        local found = {}
        local lfs = require("libs/libkoreader-lfs")
        for path_index, root in ipairs(paths) do
            if path_index == 1 or not safe_mode then
                for entry in lfs.dir(root) do
                    local name = entry:match("^(.*)%.koplugin$")
                    if name and not discovered[name] and (#names == 0 or wanted[name]) then
                        discovered[name] = true
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
    for _, module in ipairs(Loader.disabled_plugins or {}) do
        if module.name then M.status[module.name] = { loaded = false } end
    end
    for name, message in pairs(Loader.load_errors or {}) do failed(name, message) end
    for name in pairs(discovered) do
        if not M.status[name] then M.status[name] = { loaded = false } end
    end
    local ui = require(reader_mode and "apps/reader/readerui" or "apps/filemanager/filemanager")
    M.ui = ui
    M.reader_mode = reader_mode
    M.post_init = {}
    for _, module in ipairs(modules) do
        if reader_mode or not module.is_doc_only then
            require("kaia/plugin_compat")(module)
            local name = module.name
            local attr = { ui = ui }
            if reader_mode then
                attr.document = ui.document
                attr.view = ui.view
            end
            M.loading_name = name
            M.owner = name
            local instance_ok, instance = Loader:createPluginInstance(module, attr)
            M.loading_name = nil
            M.owner = nil
            if instance_ok and instance then
                M.instances[name] = instance
                ui[name] = instance
                M.status[name] = { loaded = true }
            else
                failed(name, instance or "plugin did not create an instance")
            end
        else
            M.status[module.name] = { loaded = false }
        end
    end
    for _, entry in ipairs(M.post_init) do guarded(entry.name, entry.callback) end
end

function M.dispatch(name, ...)
    if KAIA_READER then
        if name == "SetFontSize" then KAIA_READER.set_setting("font_size", (...)) end
        if name == "SetFont" then KAIA_READER.set_setting("font_face", (...)) end
        if name == "GotoViewRel" then
            KAIA_READER.go_to_page(math.max(1, math.min(KAIA_READER.page_count(), KAIA_READER.current_page() + (...))))
        end
    end
    local method = "on" .. name
    for plugin_name, instance in pairs(M.instances) do
        local handler = instance[method]
        if handler then
            if type(handler) == "table" and handler.f then handler = handler.f end
            if name == "DocSettingsLoad" and select("#", ...) == 0 then
                guarded(plugin_name, handler, instance, instance.ui.doc_settings, instance.ui.document)
            elseif name == "ReaderReady" and select("#", ...) == 0 then
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
    if item.sub_item_table_func then
        local ok, items = guarded(name, item.sub_item_table_func)
        if not ok then return end
        item.sub_item_table = items
        for _, child in ipairs(items) do M.wrap_menu(name, child) end
    end
    if item.sub_item_table then
        local Menu = require("kaia/plugin_menu")
        local UIManager = require("ui/uimanager")
        guarded(name, function() UIManager:show(Menu:new{ item_table = item.sub_item_table, title = item.text or (item.text_func and item.text_func()) }) end)
    elseif item.callback then
        local Menu = require("kaia/plugin_menu")
        local caller = Menu:new{ item_table = {item}, title = item.text or (item.text_func and item.text_func()) }
        guarded(name, item.callback, caller)
    else
        failed(name, "main menu entry " .. id .. " has no action")
    end
end

function M.event(event)
    local name = event.handler:sub(3)
    if name == "Gesture" or name == "KeyPress" or name == "KeyRelease" then return end
    M.dispatch(name, unpack(event.args or {}, 1, event.args and event.args.n))
end
function M.input(event)
    if event.handler ~= "onGesture" then return false end
    local ges = event.args[1]
    local Screen = require("device").screen
    for _, zone in ipairs(M.touch_zones or {}) do
        local r = zone.screen_zone
        if zone.ges == ges.ges and ges.pos.x >= r.ratio_x * Screen:getWidth()
            and ges.pos.x < (r.ratio_x + r.ratio_w) * Screen:getWidth()
            and ges.pos.y >= r.ratio_y * Screen:getHeight()
            and ges.pos.y < (r.ratio_y + r.ratio_h) * Screen:getHeight() then
            local ok, used = guarded("gestures", zone.handler, ges)
            if ok and used then return true end
        end
    end
    return false
end
return M
