local library = KAIA_LIBRARY
if not library then error("FileManager needs a LibraryHost", 2) end
local ui = {
    path = library.current_folder(),
    files = library.files(),
    openFile = function(_, path) return library.open_book(path) end,
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
    error("FileManager." .. tostring(key) .. " is unavailable", 2)
end })
