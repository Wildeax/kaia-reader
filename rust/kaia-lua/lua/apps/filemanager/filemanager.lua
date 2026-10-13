local library = KAIA_LIBRARY
local function connected()
    if not library then error("FileManager needs a LibraryHost", 2) end
    return library
end
local ui = {
    registerTouchZones = function(_, zones)
        for _, zone in ipairs(zones) do table.insert(KAIA_PLUGIN.touch_zones, zone) end
    end,
    registerPostInitCallback = function(_, callback)
        table.insert(KAIA_PLUGIN.post_init, {name=KAIA_PLUGIN.loading_name, callback=callback})
    end,
    openFile = function(_, path) return connected().open_book(path) end,
    copyFileFromTo = function(_, source, folder) return connected().copy_book(source, folder) end,
    moveFile = function(_, source, folder) return connected().move_book(source, folder) end,
    showFiles = function(_, path) return connected().show_folder(path) end,
    folder_shortcuts = {
        registerShortcut = function(shortcut) KAIA_PLUGIN.shortcuts[shortcut.name] = shortcut end,
        expandPath = function(_, path) return path end,
    },
    menu = {
        registerToMainMenu = function(_, plugin)
            local items = {}
            plugin:addToMainMenu(items)
            for id, item in pairs(items) do
                KAIA_PLUGIN.wrap_menu(plugin.name, item)
                KAIA_PLUGIN.menus[id] = item
                KAIA_PLUGIN.menu_owners[id] = plugin.name
            end
        end,
    },
}
return setmetatable(ui, { __index = function(_, key)
    if key == "path" then return library and library.current_folder() end
    if key == "files" then return library and library.files() or {} end
    if key == "instance" or key == "document" or key == "view" or key == "dictionary" or key == "link" or key == "profiles" then return nil end
    error("FileManager." .. tostring(key) .. " is unavailable", 2)
end })
