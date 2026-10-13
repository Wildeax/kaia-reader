local function strict(label, object)
    return setmetatable(object, { __index = function(_, key)
        error(label .. "." .. tostring(key) .. " is unavailable", 2)
    end })
end

local reader = KAIA_READER
if not reader then error("ReaderUI needs a ReaderHost", 2) end

local props = reader.properties()
local is_pdf = reader.file():lower():match("%.pdf$") ~= nil
if props.has_pages == nil then props.has_pages = is_pdf end

local document = strict("document", {
    file = reader.file(),
    is_pdf = is_pdf,
    is_pic = false,
    info = props,
    getPageCount = function() return reader.page_count() end,
    getCurrentPage = function() return reader.current_page() end,
    getPageText = function(_, page) return reader.page_text(page) end,
    getTextFromPositions = function(_, a, b)
        return { text = reader.text_from_positions(a, b), sboxes = reader.boxes_from_positions(a, b) }
    end,
    getScreenBoxesFromPositions = function(_, a, b) return reader.boxes_from_positions(a, b) end,
    getXPointer = function() return reader.xpointer_for_page(reader.current_page()) end,
    getPageFromXPointer = function(_, xp) return reader.page_for_xpointer(xp) end,
    getPageMargins = function(_, page) return reader.page_margins(page) end,
    getNativePageDimensions = function(_, page) return reader.native_page_dimensions(page) end,
    getProps = function() return reader.properties() end,
})

local settings = strict("doc_settings", {
    readSetting = function(_, key, default)
        local value = reader.get_setting(key)
        if value == nil then
            if key == "summary" then return default or {} end
            if key == "annotations" then return reader.annotations() end
            if key == "percent_finished" then return reader.current_page() / math.max(reader.page_count(), 1) end
            if key == "doc_props" then return reader.properties() end
            if key == "doc_pages" then return reader.page_count() end
            if key == "doc_path" then return reader.file() end
            return default
        end
        return value
    end,
    saveSetting = function(_, key, value) reader.set_setting(key, value) end,
    delSetting = function(_, key) reader.set_setting(key, nil) end,
    has = function(_, key) return reader.get_setting(key) ~= nil end,
    isTrue = function(_, key) return reader.get_setting(key) == true end,
    isFalse = function(_, key) return reader.get_setting(key) == false end,
    nilOrFalse = function(_, key) return reader.get_setting(key) ~= true end,
    nilOrTrue = function(_, key) return reader.get_setting(key) ~= false end,
    makeTrue = function(_, key) reader.set_setting(key, true) end,
    makeFalse = function(_, key) reader.set_setting(key, false) end,
})

local pages = strict("paging", {
    gotoPage = function(_, page) reader.go_to_page(page) end,
    getCurrentPage = function() return reader.current_page() end,
    getLastPercent = function() return reader.current_page() / math.max(reader.page_count(), 1) end,
    getLastProgress = function()
        if is_pdf then return reader.current_page() end
        return reader.xpointer_for_page(reader.current_page())
    end,
})

local annotation = strict("annotation", {
    annotations = reader.annotations(),
    hasAnnotations = function(self)
        self.annotations = reader.annotations()
        return #self.annotations > 0
    end,
    getNumberOfHighlightsAndNotes = function(self)
        self.annotations = reader.annotations()
        local highlights, notes = 0, 0
        for _, item in ipairs(self.annotations) do
            if item.drawer then highlights = highlights + 1 end
            if item.note then notes = notes + 1 end
        end
        return highlights, notes
    end,
    updatePageNumbers = function(self) self.annotations = reader.annotations() end,
})

local view = strict("view", {
    registerViewModule = function(_, module)
        -- The foreign overlay is recorded until the reader paints it.
        KAIA_PLUGIN.view_modules[#KAIA_PLUGIN.view_modules + 1] = module
    end,
    footer = strict("view.footer", { maybeUpdateFooter = function() end }),
})

local menu = strict("menu", {
    registerToMainMenu = function(_, plugin)
        local items = {}
        plugin:addToMainMenu(items)
        for id, item in pairs(items) do
            KAIA_PLUGIN.wrap_menu(plugin.name, item)
            KAIA_PLUGIN.menus[id] = item
            KAIA_PLUGIN.menu_owners[id] = plugin.name
        end
    end,
})

local ui = {
    document = document,
    doc_settings = settings,
    doc_props = props,
    paging = pages,
    rolling = pages,
    view = view,
    highlight = strict("highlight", {
        addToHighlightDialog = function(_, id, callback)
            KAIA_PLUGIN.highlight_dialog[id] = callback
        end,
    }),
    annotation = annotation,
    bookmark = strict("bookmark", {}),
    toc = strict("toc", { toc = reader.toc() }),
    menu = menu,
    folder_shortcuts = strict("folder_shortcuts", {
        registerShortcut = function(shortcut)
            KAIA_PLUGIN.shortcuts[shortcut.name] = shortcut
        end,
        updateShortcut = function(_, name, path)
            local shortcut = KAIA_PLUGIN.shortcuts[name]
            if not shortcut then error("folder_shortcuts.updateShortcut: unknown shortcut " .. tostring(name), 2) end
            shortcut.set(path)
        end,
    }),
}

ui.bookinfo = strict("bookinfo", {
    expandString = function(_, text) return text end,
    extendProps = function(_, props) return props end,
})

return setmetatable(ui, { __index = function(_, key)
    if key == "cloudstorage" or key == "pagemap" then return nil end
    error("ReaderUI." .. tostring(key) .. " is unavailable", 2)
end })
