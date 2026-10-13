-- Screenshot action for the Kaia stickers plugin, paged documents. Run it
-- through tools/kaia/screenshot.sh with a delay of about 12 seconds:
--
--   tools/kaia/screenshot.sh /tmp/kaia-stickers/pdf.png tools/kaia/shot/stickers-pdf.lua 12
--
-- It opens a PDF with a text layer (KAIA_SHOT_PDF, default the paper.pdf test
-- fixture), selects two lines of text and adds a fixed sticker on each line
-- through the plugin API. Only runs through screenshot.sh, which sets KAIA_SHOT_*.

local Screen = require("device").screen
local ReaderUI = require("apps/reader/readerui")
local UIManager = require("ui/uimanager")

local PDF = os.getenv("KAIA_SHOT_PDF") or "spec/front/unit/data/paper.pdf"
local ASSETS = {
    "resources/kaia/stickers/star.png",
    "resources/kaia/stickers/heart.png",
}

-- The text of the line at a screen row, as a selection. Returns the two
-- positions, or nil when the row has no text.
local function selectionAtRow(readerui, y)
    local view = readerui.view
    local a = view:screenToPageTransform({ x = 80, y = y })
    local b = view:screenToPageTransform({ x = Screen:getWidth() - 80, y = y + 10 })
    local res = readerui.document:getTextFromPositions(a, b)
    if not (res and res.text and res.text ~= "" and res.pos0 and res.pos1) then
        return nil
    end
    return res.pos0, res.pos1
end

local function addDemoStickers(readerui)
    readerui.view:paintTo(Screen.bb, 0, 0)
    local rows = { Screen:getHeight() * 0.35, Screen:getHeight() * 0.58 }
    for i, row in ipairs(rows) do
        local pos0, pos1 = selectionAtRow(readerui, row)
        if pos0 then
            local record, msg = readerui.kaiastickers:addStickerAt(pos0, pos1, "", ASSETS[i])
            if not record then
                io.stderr:write("kaia-shot stickers-pdf: ", tostring(msg), "\n")
            end
        else
            io.stderr:write("kaia-shot stickers-pdf: no text on row ", tostring(row), "\n")
        end
    end
    UIManager:setDirty(readerui.dialog, "full")
end

ReaderUI:showReader(PDF)

local function waitForReader(tries)
    local readerui = ReaderUI.instance
    if readerui and readerui.kaiastickers and readerui.kaiastickers.store then
        UIManager:scheduleIn(2, function()
            addDemoStickers(readerui)
        end)
    elseif tries > 0 then
        UIManager:scheduleIn(0.5, function()
            waitForReader(tries - 1)
        end)
    else
        io.stderr:write("kaia-shot stickers-pdf: the reader did not open with stickers\n")
    end
end

UIManager:scheduleIn(1, function()
    waitForReader(20)
end)
