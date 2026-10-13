-- KR-006 workload: a fixed script that measures reading latency and memory and
-- writes the results as JSON. It runs the same steps on the desktop emulator
-- (through tools/kaia/bench/run.sh) and on a device (through a flag file).
--
-- It does nothing unless an output path is set. The path comes from the
-- environment variable KAIA_BENCH_OUT, or from a line `KAIA_BENCH_OUT=...` in
-- <data dir>/kaia-bench.conf. The conf file is the device route, because a
-- Kobo has no shell environment for the launcher. Other settings use the same
-- rule: the environment first, then the conf file, then the default.
--
-- Steps, in order:
--   1. Kaia Home first paint (show() to the next completed repaint).
--   2. Kaia Notes first paint.
--   3. Cold open of the book (showReader() to the ReaderReady event).
--   4. Page turns: 5 warm-up turns, then 50 measured turns. A turn runs from
--      the GotoViewRel event to the end of the next UIManager repaint.
--   5. One search for a word (engine search, time to the first hit).
--   6. Highlights: selection to saved annotation, then the settings flush.
--   7. Memory: Lua heap and process RSS.
--
-- The JSON holds timings, counts and memory only. It never holds book text, and
-- the search term is a benchmark setting, not text from the book.
--
-- Settings (environment, or conf file):
--   KAIA_BENCH_BOOK      full path of the book to open (required)
--   KAIA_BENCH_SEARCH    search term (default "the")
--   KAIA_BENCH_SETTLE    seconds to wait after startup (default 3)
--   KAIA_BENCH_TIMEOUT   watchdog for the whole run, seconds (default 300)
--   KAIA_BENCH_MODE      label written to the JSON, for example baseline or kaia
--   KAIA_BENCH_RUN       run number label
--   KAIA_BENCH_COMMIT    commit label
-- Stored as a file named by the late priority prefix: 2-kaia-bench.lua.

local DataStorage = require("datastorage")

local function readConf()
    local conf = {}
    local f = io.open(DataStorage:getDataDir() .. "/kaia-bench.conf", "r")
    if not f then
        return conf
    end
    for line in f:lines() do
        if not line:match("^%s*#") then
            local key, value = line:match("^%s*([%w_]+)%s*=%s*(.-)%s*$")
            if key then
                conf[key] = value
            end
        end
    end
    f:close()
    return conf
end

local conf = readConf()

local function setting(name, default)
    local env = os.getenv(name)
    if env and env ~= "" then
        return env
    end
    local value = conf[name]
    if value and value ~= "" then
        return value
    end
    return default
end

local out = setting("KAIA_BENCH_OUT")
if not out then
    return
end

local UIManager = require("ui/uimanager")
local Device = require("device")
local Screen = Device.screen
local Event = require("ui/event")
local time = require("ui/time")
local ReaderUI = require("apps/reader/readerui")
-- Kaia modules are optional: a stock KOReader build has none of them. The
-- Kaia-only steps are skipped and their metrics are left out.
local okHome, Home = pcall(require, "kaia/home")
local okNotes, Notes = pcall(require, "kaia/notes")

local WARMUP_TURNS = 5
local MEASURED_TURNS = 50
local HIGHLIGHT_TARGET = 5
local HIGHLIGHT_ROWS = { 0.15, 0.25, 0.35, 0.45, 0.55, 0.65, 0.75, 0.85 }
local REPAINT_TIMEOUT = 10
local OPEN_TIMEOUT = 120
local search_term = setting("KAIA_BENCH_SEARCH", "the")
local book = setting("KAIA_BENCH_BOOK")
local settle = tonumber(setting("KAIA_BENCH_SETTLE", "3")) or 3
local watchdog = tonumber(setting("KAIA_BENCH_TIMEOUT", "300")) or 300

local results = {
    schema = 1,
    bench = "kaia-kr006",
    mode = setting("KAIA_BENCH_MODE", "unset"),
    run = tonumber(setting("KAIA_BENCH_RUN", "")) or setting("KAIA_BENCH_RUN", ""),
    commit = setting("KAIA_BENCH_COMMIT", ""),
    book = book and book:match("([^/\\]+)$") or "",
    platform = Device:isEmulator() and "emulator" or "device",
    model = tostring(Device:info()),
    screen = { w = Screen:getWidth(), h = Screen:getHeight() },
    search_term = search_term,
    turns = { warmup = WARMUP_TURNS, measured = MEASURED_TURNS, moved = 0 },
    metrics = {},
    errors = {},
    finished = "running",
}

local finished = false
local pending      -- repaint measurement in progress: { t0, done }
local readyWaiter  -- called with the cold open time when ReaderReady fires
local readyStart
local readerui

local function now()
    return time.now()
end

local function toMs(delta)
    return time.to_us(delta) / 1000
end

local function fail(message)
    table.insert(results.errors, message)
    io.stderr:write("kaia-bench: ", message, "\n")
end

local function writeResults(reason)
    if finished then
        return
    end
    finished = true
    results.finished = reason
    local encode
    local function encodeString(s)
        local escapes = { ['"'] = '\\"', ["\\"] = "\\\\", ["\n"] = "\\n", ["\r"] = "\\r", ["\t"] = "\\t" }
        return '"' .. (s:gsub('[%c"\\]', function(c)
            return escapes[c] or string.format("\\u%04x", c:byte())
        end)) .. '"'
    end
    local function encodeNumber(v)
        if v ~= v or v == math.huge or v == -math.huge then
            return "null"
        end
        if math.floor(v) == v then
            return string.format("%.0f", v)
        end
        return string.format("%.3f", v)
    end
    encode = function(v)
        local kind = type(v)
        if kind == "number" then
            return encodeNumber(v)
        elseif kind == "string" then
            return encodeString(v)
        elseif kind == "boolean" then
            return v and "true" or "false"
        elseif kind == "table" then
            if v[1] ~= nil or next(v) == nil then
                local parts = {}
                for _, item in ipairs(v) do
                    table.insert(parts, encode(item))
                end
                return "[" .. table.concat(parts, ",") .. "]"
            end
            local keys = {}
            for key in pairs(v) do
                table.insert(keys, tostring(key))
            end
            table.sort(keys)
            local parts = {}
            for _, key in ipairs(keys) do
                table.insert(parts, encodeString(key) .. ":" .. encode(v[key]))
            end
            return "{" .. table.concat(parts, ",") .. "}"
        end
        return "null"
    end

    local tmp = out .. ".tmp"
    local f = io.open(tmp, "w")
    if f then
        f:write(encode(results), "\n")
        f:close()
        os.rename(tmp, out)
    else
        io.stderr:write("kaia-bench: cannot write ", out, "\n")
    end
    UIManager:quit()
end

-- Process RSS in KB. Linux reads /proc/self/statm (4 KiB pages assumed). macOS
-- asks ps for the parent process, which is the emulator, because popen runs
-- the command in a shell that is a child of the emulator.
local function rssKb()
    local statm = io.open("/proc/self/statm", "r")
    if statm then
        local line = statm:read("*l") or ""
        statm:close()
        local resident = tonumber(line:match("^%d+%s+(%d+)"))
        if resident then
            return resident * 4, "proc_statm"
        end
    end
    local ps = io.popen("ps -o rss= -p $PPID 2>/dev/null")
    if ps then
        local value = tonumber((ps:read("*a") or ""):match("%d+"))
        ps:close()
        if value then
            return value, "ps"
        end
    end
    return nil, nil
end

-- Wrap UIManager:_repaint. A measurement ends when the first repaint that
-- completes after its start returns. The callback runs on the next tick, so
-- the next step is outside the timed window.
local orig_repaint = UIManager._repaint
function UIManager:_repaint(...)
    local result = orig_repaint(self, ...)
    local p = pending
    if p then
        pending = nil
        UIManager:nextTick(p.done, toMs(now() - p.t0))
    end
    return result
end

-- Runs start(), then calls done(ms) at the end of the next repaint, or done(nil)
-- after REPAINT_TIMEOUT seconds. Drains older repaints first, so they do not
-- end this measurement.
local function measureRepaint(start, done)
    UIManager:forceRePaint()
    local p = { t0 = now(), done = done }
    pending = p
    local ok, err = pcall(start)
    if not ok then
        pending = nil
        fail("measurement start failed: " .. tostring(err))
        UIManager:nextTick(done, nil)
        return
    end
    UIManager:scheduleIn(REPAINT_TIMEOUT, function()
        if pending == p then
            pending = nil
            fail("no repaint after " .. REPAINT_TIMEOUT .. " s")
            done(nil)
        end
    end)
end

-- Wrap ReaderUI's event handling for ReaderReady. Only the class is touched,
-- and only when a cold open waits for it.
local orig_handleEvent = ReaderUI.handleEvent
function ReaderUI:handleEvent(event)
    if event.handler == "onReaderReady" and readyWaiter then
        local cb = readyWaiter
        readyWaiter = nil
        UIManager:nextTick(cb, toMs(now() - readyStart))
    end
    return orig_handleEvent(self, event)
end

local samples = { page_turn_ms = {}, highlight_create_ms = {}, highlight_save_ms = {} }
local failedTurns = 0
local movedTurns = 0
local highlightSkipped = 0
local metrics = results.metrics

local function finishWith(reason)
    for key, list in pairs(samples) do
        if #list > 0 then
            metrics[key] = list
        end
    end
    results.turns.failed = failedTurns
    results.highlight_skipped = highlightSkipped
    writeResults(reason)
end

local function step_memory()
    metrics.lua_kb = collectgarbage("count")
    local rss, source = rssKb()
    if rss then
        metrics.rss_kb = rss
        results.rss_source = source
    else
        results.rss_source = "unavailable"
    end
    collectgarbage("collect")
    metrics.lua_kb_after_gc = collectgarbage("count")
    results.turns.moved = movedTurns
    results.config = {
        libkaia = (function()
            local okRust, RustLib = pcall(require, "kaia/rustlib")
            return okRust and RustLib.load() ~= nil or false
        end)(),
        kaiastickers = readerui ~= nil and readerui.kaiastickers ~= nil,
        start_with = G_reader_settings:readSetting("start_with") or "kaia_home",
    }
    finishWith("ok")
end

local function step_highlights(index, created)
    if created >= HIGHLIGHT_TARGET or index > #HIGHLIGHT_ROWS then
        if created < HIGHLIGHT_TARGET then
            fail("only " .. created .. " highlights made")
        end
        return UIManager:nextTick(step_memory)
    end
    local hl = readerui.highlight
    local y = Screen:getHeight() * HIGHLIGHT_ROWS[index]
    local x = Screen:getWidth() / 2
    UIManager:forceRePaint()
    local t0 = now()
    local ok, saved = pcall(function()
        if not hl:highlightWordAtCoordinates(x, y) then
            return false
        end
        return hl:saveHighlight() ~= nil
    end)
    local t1 = now()
    if not ok then
        fail("highlight failed: " .. tostring(saved))
    end
    if ok and saved then
        local t2 = now()
        readerui:saveSettings()
        local t3 = now()
        table.insert(samples.highlight_create_ms, toMs(t1 - t0))
        table.insert(samples.highlight_save_ms, toMs(t3 - t2))
        created = created + 1
    else
        highlightSkipped = highlightSkipped + 1
    end
    hl:clear()
    hl.selected_text = nil
    hl.hold_pos = nil
    hl.is_word_selection = nil
    UIManager:nextTick(step_highlights, index + 1, created)
end

local function step_search()
    local t0 = now()
    local ok, found = pcall(function()
        return readerui.search:searchFromStart(search_term)
    end)
    local ms = toMs(now() - t0)
    if ok then
        metrics.search_first_ms = ms
        metrics.search_found = found and 1 or 0
    else
        fail("search failed: " .. tostring(found))
    end
    UIManager:nextTick(step_highlights, 1, 0)
end

local function step_turn(i)
    if i > WARMUP_TURNS + MEASURED_TURNS then
        return UIManager:nextTick(step_search)
    end
    local before = readerui.document:getXPointer()
    measureRepaint(function()
        readerui:handleEvent(Event:new("GotoViewRel", 1))
    end, function(ms)
        if i > WARMUP_TURNS then
            if ms then
                table.insert(samples.page_turn_ms, ms)
                if readerui.document:getXPointer() ~= before then
                    movedTurns = movedTurns + 1
                end
            else
                failedTurns = failedTurns + 1
            end
        end
        UIManager:nextTick(step_turn, i + 1)
    end)
end

local function step_open()
    readerui = nil
    readyStart = now()
    readyWaiter = function(ms)
        metrics.cold_open_ms = ms
        readerui = ReaderUI.instance
        if not readerui then
            return finishWith("no reader instance")
        end
        UIManager:scheduleIn(1, function()
            UIManager:forceRePaint()
            UIManager:nextTick(step_turn, 1)
        end)
    end
    local ok, err = pcall(ReaderUI.showReader, ReaderUI, book)
    if not ok then
        readyWaiter = nil
        return finishWith("open failed: " .. tostring(err))
    end
    UIManager:scheduleIn(OPEN_TIMEOUT, function()
        if readyWaiter then
            readyWaiter = nil
            finishWith("reader did not reach ReaderReady")
        end
    end)
end

local function step_notes()
    if not okNotes then
        return UIManager:nextTick(step_open)
    end
    Home.closeIfShown()
    local notes
    measureRepaint(function()
        notes = Notes:show()
    end, function(ms)
        metrics.notes_first_paint_ms = ms
        if notes then
            UIManager:close(notes)
        end
        Home.closeIfShown()
        UIManager:nextTick(step_open)
    end)
end

local function step_home()
    if not okHome then
        return UIManager:nextTick(step_notes)
    end
    Home.closeIfShown()
    measureRepaint(function()
        Home:show()
    end, function(ms)
        metrics.kaia_home_first_paint_ms = ms
        Home.closeIfShown()
        UIManager:nextTick(step_notes)
    end)
end

if not book or book == "" then
    fail("KAIA_BENCH_BOOK is not set")
    UIManager:scheduleIn(0, function()
        finishWith("no book")
    end)
    return
end

UIManager:scheduleIn(watchdog, function()
    finishWith("timeout")
end)

UIManager:scheduleIn(settle, step_home)
