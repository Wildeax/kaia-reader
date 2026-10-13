-- Additional status text paints through the reader's foreign overlay layer.
local footer = { settings = { item_prefix = "text" }, content = {} }
function footer:maybeUpdateFooter() require("ui/uimanager"):setDirty(nil, "ui") end
function footer:addAdditionalFooterContent(callback)
    for _, existing in ipairs(self.content) do if existing == callback then return end end
    table.insert(self.content, callback)
    KAIA_PLUGIN.view_modules.kaia_footer = self
end
function footer:removeAdditionalFooterContent(callback)
    for i = #self.content, 1, -1 do
        if self.content[i] == callback then table.remove(self.content, i) end
    end
end
function footer:paintTo(bb, x, y)
    local parts = {}
    for _, callback in ipairs(self.content) do
        local text = callback()
        if text then parts[#parts + 1] = text end
    end
    if #parts == 0 then return end
    local text = require("ui/widget/textwidget"):new{
        text = table.concat(parts, " | "), face = require("ui/font"):getFace("cfont", 16),
    }
    local size = text:getSize()
    text:paintTo(bb, x, y + require("device").screen:getHeight() - size.h)
    text:free()
end
return footer
