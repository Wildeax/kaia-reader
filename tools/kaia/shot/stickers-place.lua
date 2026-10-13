-- Screenshot action for the Kaia stickers plugin, placing an orphaned sticker.
-- Run it through tools/kaia/screenshot.sh with a delay of about 10 seconds:
--
--   tools/kaia/screenshot.sh /tmp/kaia-stickers/place.png tools/kaia/shot/stickers-place.lua 10
--
-- It opens an EPUB from the test library (KAIA_LIBRARY, default
-- ~/kaia-test-library), adds a sticker, then breaks its locator and marks it
-- orphaned, so the locator no longer resolves. It saves the book, opens Kaia
-- Notes and runs the "Place sticker" action on the orphan: the book opens at
-- the last known position and the placement banner appears. Only runs through
-- screenshot.sh, which sets KAIA_SHOT_*.

local Screen = require("device").screen
local ReaderUI = require("apps/reader/readerui")
local UIManager = require("ui/uimanager")
local lfs = require("libs/libkoreader-lfs")
local NotesData = require("kaia/notesdata")

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

-- Adds a sticker, then makes its locator stale and its state orphaned.
local function makeOrphan(readerui)
    readerui.view:paintTo(Screen.bb, 0, 0)
    local pos0, pos1 = selectionAtRow(readerui, Screen:getHeight() * 0.40, 4)
    if not pos0 then
        io.stderr:write("kaia-shot place: no text on the row\n")
        return false
    end
    local record = readerui.kaiastickers:addStickerAt(pos0, pos1, "Check this line", "resources/kaia/stickers/star.png")
    local store = readerui.kaiastickers.store
    local stored = store:get(record.id)
    stored.anchor.locator = "/body/no-such-node"
    assert(store:put(stored))
    assert(store:setState(record.id, "orphaned"))
    return true
end

local book = pickBook()
if not book then
    io.stderr:write("kaia-shot place: no EPUB in ", library, "\n")
    return
end

ReaderUI:showReader(book)

local function waitForReader(tries)
    local readerui = ReaderUI.instance
    if readerui and readerui.kaiastickers and readerui.kaiastickers.store then
        readerui.rolling:onGotoPercent(25)
        UIManager:scheduleIn(2, function()
            if not makeOrphan(readerui) then
                return
            end
            readerui:saveSettings()
            UIManager:scheduleIn(1, function()
                local notes = require("kaia/notes"):show()
                local orphan = assert(NotesData.stickers().orphaned[1], "no orphan")
                -- The "Place sticker" button of the row menu runs this openEntry call.
                UIManager:scheduleIn(1, function()
                    notes:openEntry(orphan.file, orphan, function(ui)
                        NotesData.stickerActions.place(ui, orphan.sticker_id)
                    end)
                end)
            end)
        end)
    elseif tries > 0 then
        UIManager:scheduleIn(0.5, function()
            waitForReader(tries - 1)
        end)
    else
        io.stderr:write("kaia-shot place: the reader did not open with stickers\n")
    end
end

UIManager:scheduleIn(1, function()
    waitForReader(20)
end)
