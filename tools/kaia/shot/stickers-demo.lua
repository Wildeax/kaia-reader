-- Screenshot action for the Kaia stickers plugin. Run it through
-- tools/kaia/screenshot.sh, with a delay long enough for the reader to open
-- and the stickers to be added:
--
--   tools/kaia/screenshot.sh /tmp/stickers-demo.png tools/kaia/shot/stickers-demo.lua 12
--
-- It opens 1342.epub (or the first EPUB) from the test library, moves to a page
-- of text, and adds three stickers through the plugin API: two in the margin
-- beside their lines, and one drawn inline over its line.
-- It only runs through screenshot.sh, which sets KAIA_SHOT_*.

local Screen = require("device").screen
local ReaderUI = require("apps/reader/readerui")
local UIManager = require("ui/uimanager")
local lfs = require("libs/libkoreader-lfs")

local library = os.getenv("KAIA_LIBRARY") or (os.getenv("HOME") .. "/kaia-test-library")

local function pickBook()
    local preferred = library .. "/1342.epub"
    if lfs.attributes(preferred, "mode") == "file" then
        return preferred
    end
    local names = {}
    for name in lfs.dir(library) do
        if name:match("%.epub$") then
            names[#names + 1] = name
        end
    end
    table.sort(names)
    return names[1] and (library .. "/" .. names[1])
end

-- Selection of `words` words from the word nearest to a screen row, or nil when
-- the row has no text. Returns both selection ends as xpointers.
local function selectionAtRow(readerui, y, words)
    local doc = readerui.document
    local word = doc:getNearestWordFromPosition({ x = Screen:getWidth() / 2, y = y }, 0)
    if not (word and word.pos0 and word.pos1) then
        return nil
    end
    if #doc:getScreenBoxesFromPositions(word.pos0, word.pos1, true) == 0 then
        return nil
    end
    local pos1 = word.pos0
    for _ = 1, words do
        pos1 = doc:getNextVisibleWordEnd(pos1) or pos1
    end
    return word.pos0, pos1
end

-- Margin stickers are the default. The third sticker is moved over the text.
local PLACEMENTS = { "margin", "margin", "inline" }
local ASSETS = {
    "resources/kaia/stickers/star.png",
    "resources/kaia/stickers/heart.png",
    "resources/kaia/stickers/check.png",
}

local function addDemoStickers(readerui)
    -- Render the page first, so the engine can report word boxes.
    readerui.view:paintTo(Screen.bb, 0, 0)
    local rows = { Screen:getHeight() * 0.30, Screen:getHeight() * 0.52, Screen:getHeight() * 0.74 }
    for i, row in ipairs(rows) do
        local pos0, pos1 = selectionAtRow(readerui, row, 4)
        if pos0 then
            local record, msg = readerui.kaiastickers:addStickerAt(pos0, pos1, "", ASSETS[i])
            if not record then
                io.stderr:write("kaia-shot stickers: ", tostring(msg), "\n")
            elseif PLACEMENTS[i] == "inline" then
                local sticker = readerui.kaiastickers:findSticker(record.id)
                sticker.anchor.placement = "inline"
                sticker.anchor.offset.dy = -1.1
                readerui.kaiastickers:putSticker(sticker)
            end
        else
            io.stderr:write("kaia-shot stickers: no text on row ", tostring(row), "\n")
        end
    end
    UIManager:setDirty(readerui.dialog, "full")
end

local book = pickBook()
if not book then
    io.stderr:write("kaia-shot stickers: no EPUB in ", library, "\n")
    return
end

ReaderUI:showReader(book)

-- Opening is asynchronous. Wait for the reader, jump to a text page, let the
-- layout settle, then add the stickers. The default margins leave room for
-- the margin stickers. Margin changes are not used here, because they move the
-- view while the stickers are placed.
local function waitForReader(tries)
    local readerui = ReaderUI.instance
    if readerui and readerui.kaiastickers and readerui.kaiastickers.store then
        readerui.rolling:onGotoPercent(25)
        UIManager:scheduleIn(2, function()
            addDemoStickers(readerui)
        end)
    elseif tries > 0 then
        UIManager:scheduleIn(0.5, function()
            waitForReader(tries - 1)
        end)
    else
        io.stderr:write("kaia-shot stickers: the reader did not open with stickers\n")
    end
end

UIManager:scheduleIn(1, function()
    waitForReader(20)
end)
