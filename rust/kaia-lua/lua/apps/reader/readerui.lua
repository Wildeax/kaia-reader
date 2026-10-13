local function strict(label, object)
    return setmetatable(object, { __index = function(_, key)
        error(label .. "." .. tostring(key) .. " is unavailable", 2)
    end })
end

local reader = KAIA_READER
if not reader then return {} end

local props = reader.properties()
local is_pdf = reader.file():lower():match("%.pdf$") ~= nil
if props.has_pages == nil then props.has_pages = is_pdf end

local document = strict("document", {
    file = reader.file(),
    is_pdf = is_pdf,
    is_pic = false,
    info = props,
    configurable = setmetatable({}, { __index = function(_, key)
        local config = reader.get_setting("configurable") or {}
        return config[key]
    end }),
    getPageCount = function() return reader.page_count() end,
    getCurrentPage = function() return reader.current_page() end,
    getVisiblePageCount = function() return 1 end,
    getPageText = function(_, page) return reader.page_text(page) end,
    getTextFromPositions = function(_, a, b)
        return { text = reader.text_from_positions(a, b), sboxes = reader.boxes_from_positions(a, b) }
    end,
    getScreenBoxesFromPositions = function(_, a, b) return reader.boxes_from_positions(a, b) end,
    getXPointer = function() return reader.xpointer_for_page(reader.current_page()) end,
    getPageFromXPointer = function(_, xp) return reader.page_for_xpointer(xp) end,
    getPageMargins = function(_, page) return reader.page_margins(page) end,
    getNativePageDimensions = function(_, page) return reader.native_page_dimensions(page) end,
    hasHiddenFlows = function() return false end,
    getTotalPagesLeft = function(_, page) return reader.page_count() - page end,
    getProps = function() return reader.properties() end,
})

local settings = strict("doc_settings", {
    data = { summary = reader.get_setting("summary") or {} },
    readSetting = function(self, key, default)
        local value = self.data[key]
        if value == nil then value = reader.get_setting(key) end
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
    saveSetting = function(self, key, value) self.data[key] = value; reader.set_setting(key, value) end,
    delSetting = function(self, key) self.data[key] = nil; reader.set_setting(key, nil) end,
    flush = function(self)
        for key, value in pairs(self.data) do reader.set_setting(key, value) end
        return reader.flush_settings()
    end,
    hasNot = function(self, key) return self:readSetting(key) == nil end,
    has = function(self, key) return self:readSetting(key) ~= nil end,
    isTrue = function(self, key) return self:readSetting(key) == true end,
    isFalse = function(self, key) return self:readSetting(key) == false end,
    nilOrFalse = function(self, key) return self:readSetting(key) ~= true end,
    nilOrTrue = function(self, key) return self:readSetting(key) ~= false end,
    makeTrue = function(self, key) self:saveSetting(key, true) end,
    makeFalse = function(self, key) self:saveSetting(key, false) end,
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
    flipping = false,
    state = setmetatable({}, { __index = function(_, key)
        if key == "page" then return reader.current_page() end
        error("view.state." .. tostring(key) .. " is unavailable", 2)
    end }),
    registerViewModule = function(_, name, module)
        KAIA_PLUGIN.view_modules[name] = module
    end,
    footer = require("kaia/reader_footer"),
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
    name = "ReaderUI",
    invisible = false,
    reloading = false,
    tearing_down = false,
    switchDocument = function(_, file)
        return require("apps/filemanager/filemanager"):openFile(file)
    end,
    document = document,
    doc_settings = settings,
    doc_props = props,
    paging = is_pdf and pages or nil,
    rolling = not is_pdf and pages or nil,
    getCurrentPage = function() return reader.current_page() end,
    registerTouchZones = function(_, zones)
        KAIA_PLUGIN.touch_zones = KAIA_PLUGIN.touch_zones or {}
        for _, zone in ipairs(zones) do table.insert(KAIA_PLUGIN.touch_zones, zone) end
    end,
    registerPostInitCallback = function(_, callback)
        table.insert(KAIA_PLUGIN.post_init, {name=KAIA_PLUGIN.loading_name, callback=callback})
    end,
    handleEvent = function(_, event)
        return KAIA_PLUGIN.dispatch(event.handler:sub(3), unpack(event.args or {}))
    end,
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
    font = setmetatable({}, { __index = function(_, key)
        if key == "font_face" then return reader.get_setting("font_face") end
        error("font." .. tostring(key) .. " is unavailable", 2)
    end }),
    folder_shortcuts = strict("folder_shortcuts", {
        expandPath = function(_, path) return path end,
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

require("kaia/reader_modules")(ui, reader, strict)

return setmetatable(ui, { __index = function(_, key)
    if key == "cloudstorage" or key == "pagemap" or key == "profiles"
        or key == "dictionary" or key == "paging" or key == "rolling"
        or key == "statistics" or key == "typeset" or key == "vocabbuilder" or key == "disable_double_tap" or key == "link" or key == "file_chooser" then return nil end
    error("ReaderUI." .. tostring(key) .. " is unavailable", 2)
end })
