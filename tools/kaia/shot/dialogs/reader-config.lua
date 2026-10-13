-- Screenshot action for the dialog restyle check (KR-013). Opens 1342.epub from
-- the test library (or the first EPUB) and shows the reader's bottom config
-- panel. Run with a delay of about 12 seconds:
--
--   tools/kaia/screenshot.sh /tmp/kaia-dialogs/reader-config.png \
--       tools/kaia/shot/dialogs/reader-config.lua 12
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

local function waitForConfig(tries)
    local readerui = ReaderUI.instance
    if readerui and readerui.config and readerui.document then
        readerui.config:onShowConfigMenu()
    elseif tries > 0 then
        UIManager:scheduleIn(0.5, function()
            waitForConfig(tries - 1)
        end)
    else
        io.stderr:write("kaia-shot reader-config: the reader did not open\n")
    end
end

local book = pickBook()
if not book then
    io.stderr:write("kaia-shot reader-config: no EPUB in ", library, "\n")
    return
end

require("kaia/home").closeIfShown()
ReaderUI:showReader(book)
UIManager:scheduleIn(1, function()
    waitForConfig(20)
end)
