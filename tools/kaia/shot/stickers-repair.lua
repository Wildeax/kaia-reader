-- Screenshot action for the Kaia stickers plugin, orphaned stickers in Notes.
-- Run it through tools/kaia/screenshot.sh with a delay of about 14 seconds:
--
--   tools/kaia/screenshot.sh /tmp/kaia-stickers/repair.png tools/kaia/shot/stickers-repair.lua 14
--
-- It opens an EPUB from the test library, adds a sticker on a line of text,
-- marks it orphaned, saves the book's sidecar, and opens Kaia Notes over the
-- reader. The sticker is listed under "Needs repair". Only runs through
-- screenshot.sh, which sets KAIA_SHOT_*.

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

local function markOrphan(readerui)
    readerui.view:paintTo(Screen.bb, 0, 0)
    local pos0, pos1 = selectionAtRow(readerui, Screen:getHeight() * 0.40, 4)
    if not pos0 then
        io.stderr:write("kaia-shot repair: no text on the row\n")
        return false
    end
    local record, msg = readerui.kaiastickers:addStickerAt(pos0, pos1, "Check this line", "resources/kaia/stickers/star.png")
    if not record then
        io.stderr:write("kaia-shot repair: ", tostring(msg), "\n")
        return false
    end
    local ok, err = readerui.kaiastickers.store:setState(record.id, "orphaned")
    if not ok then
        io.stderr:write("kaia-shot repair: setState failed ", tostring(err), "\n")
        return false
    end
    return true
end

local book = pickBook()
if not book then
    io.stderr:write("kaia-shot repair: no EPUB in ", library, "\n")
    return
end

ReaderUI:showReader(book)

local function waitForReader(tries)
    local readerui = ReaderUI.instance
    if readerui and readerui.kaiastickers and readerui.kaiastickers.store then
        readerui.rolling:onGotoPercent(25)
        UIManager:scheduleIn(2, function()
            if markOrphan(readerui) then
                -- Notes finds the book through its sidecar, so write it now. The
                -- reader stays open underneath, because closing the last widget
                -- would quit the emulator.
                readerui:saveSettings()
                UIManager:scheduleIn(1, function()
                    require("kaia/notes"):show()
                end)
            end
        end)
    elseif tries > 0 then
        UIManager:scheduleIn(0.5, function()
            waitForReader(tries - 1)
        end)
    else
        io.stderr:write("kaia-shot repair: the reader did not open with stickers\n")
    end
end

UIManager:scheduleIn(1, function()
    waitForReader(20)
end)
