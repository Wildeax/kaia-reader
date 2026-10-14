-- Pencil controls use the native ink owner. Its framebuffer and input patches
-- cannot run alongside the Rust reader. The pinned menu callbacks stay in use.
return function(module)
    local menu = module.addToMainMenu
    local function command(action) KAIA_DEVICE("ink", action) end
    function module:init()
        assert(KAIA_DEVICE, "Pencil needs the native ink host")
        self.available_widths = {}
        self:loadSettings()
        self.current_tool = "pen"
        self.ui.menu:registerToMainMenu(self)
        local Dispatcher = require("dispatcher")
        for _, item in ipairs({
            {"pencil_toggle_enabled", "PencilToggleEnabled", "Pencil: toggle on/off"},
            {"pencil_toggle_tool", "PencilToggleTool", "Pencil: toggle pencil/eraser"},
            {"pencil_select_pen", "PencilSelectPen", "Pencil: select pencil"},
            {"pencil_select_eraser", "PencilSelectEraser", "Pencil: select eraser"},
            {"pencil_undo", "PencilUndo", "Pencil: undo"},
        }) do
            Dispatcher:registerAction(item[1], {category="none", event=item[2],
                title=require("gettext")(item[3]), reader=true})
        end
    end
    function module:addToMainMenu(items)
        menu(self, items)
        local children = items.pencil_annotation.sub_item_table
        items.pencil_annotation.sub_item_table = {
            children[1], children[3],
            {text=require("gettext")("Undo"), callback=function() self:onPencilUndo() end},
        }
    end
    function module:setupPenInput()
        command("pencil-on")
        self:setTool(self.current_tool)
    end
    function module:setEnabled(enabled)
        G_reader_settings:saveSetting("pencil_annotation_enabled", enabled)
        G_reader_settings:flush()
    end
    function module:teardownPenInput() command("pencil-off") end
    function module:setTool(tool)
        self.current_tool = tool
        command(tool == "eraser" and "pencil-eraser" or "pencil-pen")
    end
    for _, name in ipairs({"onPencilSelectPen", "onPencilSelectEraser", "onPencilToggleTool"}) do
        local original = module[name]
        module[name] = function(self, ...)
            local result = original(self, ...)
            self:setTool(self.current_tool)
            return result
        end
    end
    function module:undoLastStroke() command("undo") end
    function module:onReaderReady()
        if self:isEnabled() then self:setupPenInput() end
    end
    -- Native ink saves each completed stroke and cancels incomplete strokes.
    function module:onPageUpdate() end
    function module:onUpdatePos() end
    function module:onReadSettings() end
    function module:onCloseDocument() self:teardownPenInput() end
    function module:onCloseWidget() self:teardownPenInput() end
    function module:onSuspend() end
end
