describe("kaia importko", function()
    local ImportKO, LuaSettings, SQ3, lfs, util, ffiutil
    local root

    local function writeFile(path, content)
        util.makePath(path:match("^(.*)/[^/]+$"))
        local f = assert(io.open(path, "wb"))
        f:write(content)
        f:close()
    end

    local function readFile(path)
        local f = io.open(path, "rb")
        if not f then return nil end
        local content = f:read("*a")
        f:close()
        return content
    end

    -- Sorted "rel|mode|size|mtime" lines for every entry below dir.
    local function snapshot(dir)
        local lines = {}
        local function walk(path, rel)
            for name in lfs.dir(path) do
                if name ~= "." and name ~= ".." then
                    local full = path .. "/" .. name
                    local attr = lfs.attributes(full)
                    table.insert(lines, string.format("%s|%s|%s|%s",
                        rel .. name, attr.mode, attr.size, attr.modification))
                    if attr.mode == "directory" then
                        walk(full, rel .. name .. "/")
                    end
                end
            end
        end
        walk(dir, "")
        table.sort(lines)
        return lines
    end

    local function countFiles(dir, pattern)
        local found = {}
        local function walk(path)
            for name in lfs.dir(path) do
                if name ~= "." and name ~= ".." then
                    local full = path .. "/" .. name
                    if lfs.attributes(full, "mode") == "directory" then
                        walk(full)
                    elseif name:find(pattern) then
                        table.insert(found, full)
                    end
                end
            end
        end
        walk(dir)
        return found
    end

    -- A KOReader data directory with one of each kind of item.
    local function populateKoreader(src)
        writeFile(src .. "/settings.reader.lua", [[return {
    ["font_size"] = 18,
    ["ui_theme"] = "dark",
    ["start_with"] = "filemanager",
    ["document_metadata_folder"] = "doc",
    ["device_id"] = "old-device",
    ["kaia_import_offer_shown"] = true,
    ["ota_channel"] = "stable",
    ["quickstart_shown_version"] = "2026.07",
    ["lastdir"] = "/does/not/exist",
    ["home_dir"] = "]] .. src .. [[/books",
}
]])
        writeFile(src .. "/history.lua", [[return {
    { time = 1700000100, file = "/books/b.epub" },
    { time = 1700000000, file = "/books/a.epub" },
}
]])
        writeFile(src .. "/docsettings/books/a.sdr/metadata.epub.lua", 'return { percent_finished = 0.4 }\n')
        writeFile(src .. "/docsettings/books/a.sdr/cover.jpg", "jpeg-bytes")
        writeFile(src .. "/docsettings/books/a.sdr/custom_metadata.lua", 'return { title = "A" }\n')
        writeFile(src .. "/docsettings/books/a.sdr/notes.txt", "not a sidecar")
        writeFile(src .. "/docsettings/books/stray.epub", "a book in the wrong place")
        writeFile(src .. "/docsettings/books/metadata.epub.lua", "not in an .sdr folder")
        writeFile(src .. "/hashdocsettings/b3/b3fb0000.sdr/metadata.pdf.lua", 'return { percent_finished = 0.9 }\n')
        writeFile(src .. "/settings/statistics.sqlite3", "stats-db")
        writeFile(src .. "/settings/vocabulary_builder.sqlite3", "vocab-db")
        writeFile(src .. "/settings/bookinfo_cache.sqlite3", "cache-db")
        writeFile(src .. "/settings/bookinfo_cache.sqlite3-journal", "cache-journal")
        writeFile(src .. "/settings/gestures.lua", "return { }\n")
        writeFile(src .. "/settings/kosync_queue.lua", "return { }\n")
        writeFile(src .. "/styletweaks/reader.css", "body { }\n")
        writeFile(src .. "/patches/2-my-patch.lua", "-- user code\n")
        writeFile(src .. "/plugins/mything.koplugin/main.lua", "-- user plugin\n")
        writeFile(src .. "/screenshots/shot.png", "png")
        writeFile(src .. "/books/a.epub", "book bytes")
    end

    setup(function()
        require("commonrequire")
        ImportKO = require("kaia/importko")
        LuaSettings = require("luasettings")
        lfs = require("libs/libkoreader-lfs")
        util = require("util")
        ffiutil = require("ffi/util")
        SQ3 = require("lua-ljsqlite3/init")
    end)

    before_each(function()
        root = os.tmpname()
        os.remove(root)
        lfs.mkdir(root)
    end)

    after_each(function()
        ffiutil.purgeDir(root)
    end)

    describe("findSource", function()
        it("finds the sibling koreader directory of a Kobo layout", function()
            local src = root .. "/.adds/koreader"
            util.makePath(src)
            writeFile(src .. "/settings.reader.lua", "return {}\n")
            assert.are.equal(src, ImportKO.findSource{ dest = root .. "/.adds/kaia" })
        end)

        it("finds the sibling koreader directory of a desktop layout", function()
            local src = root .. "/Library/koreader"
            util.makePath(src)
            writeFile(src .. "/settings.reader.lua", "return {}\n")
            assert.are.equal(src, ImportKO.findSource{ dest = root .. "/Library/kaia" })
        end)

        it("returns nil when the sibling has no settings.reader.lua", function()
            util.makePath(root .. "/.adds/koreader")
            assert.is_nil(ImportKO.findSource{ dest = root .. "/.adds/kaia" })
        end)

        it("returns nil when there is no sibling directory", function()
            assert.is_nil(ImportKO.findSource{ dest = root .. "/.adds/kaia" })
        end)

        it("returns nil when the source is the same directory as Kaia's", function()
            util.makePath(root .. "/kaia")
            writeFile(root .. "/kaia/settings.reader.lua", "return {}\n")
            assert.is_nil(ImportKO.findSource{ dest = root .. "/kaia", source = root .. "/kaia" })
        end)

        it("uses an explicit source path from opts", function()
            local src = root .. "/elsewhere"
            util.makePath(src)
            writeFile(src .. "/settings.reader.lua", "return {}\n")
            assert.are.equal(src, ImportKO.findSource{ dest = root .. "/kaia", source = src })
        end)
    end)

    describe("mergeSettings", function()
        it("drops identity keys and keeps Kaia's values", function()
            local ko = {
                font_size = 18,
                start_with = "filemanager",
                kaia_library_navbar = false,
                ota_channel = "stable",
                device_id = "old",
                quickstart_shown_version = "2026.07",
                lastdir = "/does/not/exist",
                home_dir = root,
                ui_theme = "dark",
            }
            local kaia = { font_size = 22, document_metadata_folder = "dir" }
            local merged, summary = ImportKO.mergeSettings(ko, kaia)
            assert.are.equal(22, merged.font_size)
            assert.are.equal("dir", merged.document_metadata_folder)
            assert.are.equal("dark", merged.ui_theme)
            assert.are.equal(root, merged.home_dir)
            assert.is_nil(merged.start_with)
            assert.is_nil(merged.kaia_library_navbar)
            assert.is_nil(merged.ota_channel)
            assert.is_nil(merged.device_id)
            assert.is_nil(merged.quickstart_shown_version)
            assert.is_nil(merged.lastdir)
            assert.are.equal(2, summary.added)
            assert.are.equal(1, summary.kept)
            assert.are.equal(6, #summary.dropped)
        end)
    end)

    describe("plan", function()
        it("lists what would be copied and writes nothing", function()
            local src, dest = root .. "/koreader", root .. "/kaia"
            populateKoreader(src)
            util.makePath(dest)
            local before = snapshot(dest)
            local plan = ImportKO.plan(src, dest)
            assert.are.same(before, snapshot(dest))

            assert.are.equal(2, plan.history_books)
            assert.are.equal(4, plan.book_files)
            assert.are.equal(4, plan.plugin_files)
            assert.is_true(plan.total_bytes > 0)
            local rels = {}
            for _i, item in ipairs(plan.items) do
                rels[item.rel] = true
            end
            assert.is_true(rels["history.lua"])
            assert.is_true(rels["docsettings/books/a.sdr/metadata.epub.lua"])
            assert.is_true(rels["hashdocsettings/b3/b3fb0000.sdr/metadata.pdf.lua"])
            assert.is_true(rels["settings/statistics.sqlite3"])
            assert.is_nil(rels["settings/bookinfo_cache.sqlite3"])
            assert.is_nil(rels["settings/kosync_queue.lua"])
            assert.is_nil(rels["docsettings/books/stray.epub"])
            assert.is_nil(rels["docsettings/books/a.sdr/notes.txt"])
            assert.is_nil(rels["docsettings/books/metadata.epub.lua"])
            assert.are.same({ "2-my-patch.lua" }, plan.patches)
            assert.are.same({ "mything.koplugin" }, plan.code_plugins)
        end)

        it("names the patches and the book count in the confirmation text", function()
            local src, dest = root .. "/koreader", root .. "/kaia"
            populateKoreader(src)
            util.makePath(dest)
            local text = ImportKO.formatPlan(ImportKO.plan(src, dest))
            assert.is_truthy(text:find("reading history for 2 books", 1, true))
            assert.is_truthy(text:find("KOReader's files are not changed.", 1, true))
            assert.is_truthy(text:find("2-my-patch.lua", 1, true))
        end)
    end)

    describe("run", function()
        it("imports statistics rows that are still in KOReader's WAL", function()
            local src, dest = root .. "/koreader", root .. "/kaia"
            writeFile(src .. "/settings.reader.lua", "return {}\n")
            util.makePath(src .. "/settings")
            local source_db = src .. "/settings/statistics.sqlite3"
            local db = SQ3.open(source_db)
            db:exec("PRAGMA journal_mode=WAL")
            db:exec("CREATE TABLE import_test (value TEXT)")
            db:exec("INSERT INTO import_test VALUES ('uncheckpointed')")
            assert.is_truthy(lfs.attributes(source_db .. "-wal"))
            local before = snapshot(src)
            local report = ImportKO.run(src, dest)
            assert.are.same({}, report.failed)
            local imported = SQ3.open(dest .. "/settings/statistics.sqlite3", "ro")
            assert.are.equal("uncheckpointed", imported:rowexec("SELECT value FROM import_test"))
            imported:close()
            assert.are.same(before, snapshot(src))
            db:close()
        end)

        it("imports settings, history, sidecars and plugin data into the right folders", function()
            local src, dest = root .. "/koreader", root .. "/kaia"
            populateKoreader(src)
            util.makePath(dest)
            local report = ImportKO.run(src, dest)

            assert.are.equal(2, report.history_books)
            assert.are.equal(readFile(src .. "/history.lua"), readFile(dest .. "/history.lua"))
            assert.are.equal("jpeg-bytes", readFile(dest .. "/docsettings/books/a.sdr/cover.jpg"))
            assert.are.equal('return { title = "A" }\n',
                readFile(dest .. "/docsettings/books/a.sdr/custom_metadata.lua"))
            assert.is_not_nil(lfs.attributes(dest .. "/hashdocsettings/b3/b3fb0000.sdr/metadata.pdf.lua"))
            assert.are.equal("stats-db", readFile(dest .. "/settings/statistics.sqlite3"))
            assert.are.equal("vocab-db", readFile(dest .. "/settings/vocabulary_builder.sqlite3"))
            assert.are.equal("body { }\n", readFile(dest .. "/styletweaks/reader.css"))

            -- Not copied: books, caches, pending sync queue, code and screenshots.
            assert.is_nil(lfs.attributes(dest .. "/docsettings/books/stray.epub"))
            assert.is_nil(lfs.attributes(dest .. "/docsettings/books/a.sdr/notes.txt"))
            assert.is_nil(lfs.attributes(dest .. "/settings/bookinfo_cache.sqlite3"))
            assert.is_nil(lfs.attributes(dest .. "/settings/bookinfo_cache.sqlite3-journal"))
            assert.is_nil(lfs.attributes(dest .. "/settings/kosync_queue.lua"))
            assert.is_nil(lfs.attributes(dest .. "/patches"))
            assert.is_nil(lfs.attributes(dest .. "/plugins"))
            assert.is_nil(lfs.attributes(dest .. "/screenshots"))
            assert.is_nil(lfs.attributes(dest .. "/books"))
            assert.are.same({ "2-my-patch.lua" }, report.not_copied.patches)
            assert.are.same({ "mything.koplugin" }, report.not_copied.plugins)
        end)

        it("merges settings without KOReader's identity keys and records the import time", function()
            local src, dest = root .. "/koreader", root .. "/kaia"
            populateKoreader(src)
            util.makePath(dest)
            local report = ImportKO.run(src, dest)
            assert.is_true(report.settings_written)

            local settings = LuaSettings:open(dest .. "/settings.reader.lua")
            assert.are.equal(18, settings:readSetting("font_size"))
            assert.are.equal("dark", settings:readSetting("ui_theme"))
            assert.are.equal(src .. "/books", settings:readSetting("home_dir"))
            assert.is_nil(settings:readSetting("start_with"))
            assert.is_nil(settings:readSetting("device_id"))
            assert.is_nil(settings:readSetting("ota_channel"))
            assert.is_nil(settings:readSetting("lastdir"))
            assert.is_nil(settings:readSetting("kaia_import_offer_shown"))
            assert.is_number(settings:readSetting("kaia_imported_from_koreader"))
        end)

        it("keeps Kaia's values and existing files", function()
            local src, dest = root .. "/koreader", root .. "/kaia"
            populateKoreader(src)
            writeFile(dest .. "/settings.reader.lua", 'return { ["font_size"] = 22, ["document_metadata_folder"] = "dir" }\n')
            writeFile(dest .. "/history.lua", "return {}\n")
            writeFile(dest .. "/settings/gestures.lua", "KAIA GESTURES\n")
            local report = ImportKO.run(src, dest)

            assert.are.equal("return {}\n", readFile(dest .. "/history.lua"))
            assert.are.equal("KAIA GESTURES\n", readFile(dest .. "/settings/gestures.lua"))
            assert.are.equal(0, report.history_books)
            local existing = {}
            for _i, rel in ipairs(report.existing) do
                existing[rel] = true
            end
            assert.is_true(existing["history.lua"])
            assert.is_true(existing["settings/gestures.lua"])

            local settings = LuaSettings:open(dest .. "/settings.reader.lua")
            assert.are.equal(22, settings:readSetting("font_size"))
            assert.are.equal("dir", settings:readSetting("document_metadata_folder"))
            assert.are.equal("dark", settings:readSetting("ui_theme"))
        end)

        it("skips files over the size limit and says so", function()
            local src, dest = root .. "/koreader", root .. "/kaia"
            populateKoreader(src)
            -- The history file is about 120 bytes, so it stays under the 200 byte limit.
            writeFile(src .. "/settings/statistics.sqlite3", string.rep("x", 300))
            util.makePath(dest)
            local opts = { max_file_size = 200 }
            local plan = ImportKO.plan(src, dest, opts)
            assert.are.equal(1, #plan.too_large)
            assert.are.equal("settings/statistics.sqlite3", plan.too_large[1].rel)
            assert.are.equal(300, plan.too_large[1].size)
            assert.is_truthy(ImportKO.formatPlan(plan):find("settings/statistics.sqlite3", 1, true))

            local report = ImportKO.run(src, dest, opts)
            assert.are.equal(1, #report.too_large)
            assert.is_nil(lfs.attributes(dest .. "/settings/statistics.sqlite3"))
        end)

        it("leaves the source directory untouched", function()
            local src, dest = root .. "/koreader", root .. "/kaia"
            populateKoreader(src)
            util.makePath(dest)
            local before = snapshot(src)
            ImportKO.run(src, dest)
            assert.are.same(before, snapshot(src))
        end)

        it("copies nothing on a second run except settings", function()
            local src, dest = root .. "/koreader", root .. "/kaia"
            populateKoreader(src)
            util.makePath(dest)
            local first = ImportKO.run(src, dest)
            assert.is_true(first.copied > 0)

            local second = ImportKO.run(src, dest)
            assert.are.equal(0, second.copied)
            assert.are.equal(0, second.bytes)
            assert.are.equal(0, second.history_books)
            assert.are.equal(first.copied, #second.existing)
            assert.is_true(second.settings_written)
            assert.are.same({}, second.failed)
        end)

        it("writes through the live settings object when one is passed", function()
            local src, dest = root .. "/koreader", root .. "/kaia"
            populateKoreader(src)
            util.makePath(dest)
            local live = LuaSettings:open(dest .. "/settings.reader.lua")
            live:saveSetting("font_size", 24)
            ImportKO.run(src, dest, { settings = live })
            assert.are.equal(24, live:readSetting("font_size"))
            assert.are.equal("dark", live:readSetting("ui_theme"))
            assert.are.equal("dark", LuaSettings:open(dest .. "/settings.reader.lua"):readSetting("ui_theme"))
        end)

        it("leaves no temp files behind", function()
            local src, dest = root .. "/koreader", root .. "/kaia"
            populateKoreader(src)
            util.makePath(dest)
            ImportKO.run(src, dest)
            assert.are.same({}, countFiles(dest, "kaia%-import%-tmp$"))
        end)
    end)
end)
