-- Screenshot action for the Kaia stickers plugin, text-only stickers. Run it
-- through tools/kaia/screenshot.sh with a delay of about 12 seconds:
--
--   tools/kaia/screenshot.sh /tmp/kaia-stickers/textnote.png tools/kaia/shot/stickers-textnote.lua 12
--
-- It opens an EPUB from the test library (KAIA_LIBRARY, default
-- ~/kaia-test-library), moves to a page of text and adds two text notes in the
-- margin beside their lines, through the plugin API. Only runs through
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

-- Selection of `words` words from the word nearest to a screen row, or nil.
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

local NOTES = {
    "Check the ending of this scene. The word choice here matters.",
    "Ask about the timing of the letter in the next chapter.",
}

local function addDemoNotes(readerui)
    readerui.view:paintTo(Screen.bb, 0, 0)
    local rows = { Screen:getHeight() * 0.30, Screen:getHeight() * 0.55 }
    for i, row in ipairs(rows) do
        local pos0, pos1 = selectionAtRow(readerui, row, 4)
        if pos0 then
            local record, msg = readerui.kaiastickers:addTextNoteAt(pos0, pos1, NOTES[i])
            if not record then
                io.stderr:write("kaia-shot textnote: ", tostring(msg), "\n")
            end
        else
            io.stderr:write("kaia-shot textnote: no text on row ", tostring(row), "\n")
        end
    end
    UIManager:setDirty(readerui.dialog, "full")
end

local book = pickBook()
if not book then
    io.stderr:write("kaia-shot textnote: no EPUB in ", library, "\n")
    return
end

ReaderUI:showReader(book)

local function waitForReader(tries)
    local readerui = ReaderUI.instance
    if readerui and readerui.kaiastickers and readerui.kaiastickers.store then
        readerui.rolling:onGotoPercent(25)
        UIManager:scheduleIn(2, function()
            addDemoNotes(readerui)
        end)
    elseif tries > 0 then
        UIManager:scheduleIn(0.5, function()
            waitForReader(tries - 1)
        end)
    else
        io.stderr:write("kaia-shot textnote: the reader did not open with stickers\n")
    end
end

UIManager:scheduleIn(1, function()
    waitForReader(20)
end)
