describe("kaia notes", function()
    local DocSettings, NotesData, Notes, Blitbuffer, Screen, lfs, ffiutil
    local dir
    local a_file, b_file, c_file, d_file, e_file, f_file, gone_file
    -- Every book file this spec creates, removed in teardown with its sidecar.
    local created = {}

    local function touch(path)
        local f = assert(io.open(path, "w"))
        f:write("")
        f:close()
        created[#created + 1] = path
        return path
    end

    local function removeBookFiles(file)
        local sidecar_dir = DocSettings:getSidecarDir(file)
        ffiutil.purgeDir(sidecar_dir)
        lfs.rmdir(sidecar_dir)
        os.remove(file)
    end

    local function readAll(path)
        local f = assert(io.open(path, "r"))
        local s = f:read("*a")
        f:close()
        return s
    end

    local function paint(widget)
        local bb = Blitbuffer.new(Screen:getWidth(), Screen:getHeight(), Blitbuffer.TYPE_BB8)
        widget:paintTo(bb, 0, 0)
        bb:free()
    end

    -- Writes a real sidecar next to `file` with DocSettings.
    local function writeSidecar(file, annotations, doc_props)
        local ds = DocSettings:open(file)
        ds:saveSetting("annotations", annotations)
        if doc_props then
            ds:saveSetting("doc_props", doc_props)
        end
        ds:flush()
    end

    local function sidecarPath(file)
        return DocSettings:findSidecarFile(file)
    end

    local function highlight(text, datetime, note)
        return {
            datetime = datetime,
            drawer = "lighten",
            color = "yellow",
            text = text,
            note = note,
            chapter = "Chapter 2",
            page = "/body/DocFragment[3]/body/p[4].0",
            pos0 = "/body/DocFragment[3]/body/p[4].0",
            pos1 = "/body/DocFragment[3]/body/p[4].20",
            pageno = 12,
        }
    end

    local function bookmark(datetime, chapter)
        return {
            datetime = datetime,
            page = "/body/DocFragment[5]/body/p[1].0",
            chapter = chapter,
            pageno = 40,
        }
    end

    setup(function()
        require("commonrequire")
        disable_plugins()
        require("document/canvascontext"):init(require("device"))
        DocSettings = require("docsettings")
        NotesData = require("kaia/notesdata")
        Notes = require("kaia/notes")
        Blitbuffer = require("ffi/blitbuffer")
        Screen = require("device").screen
        lfs = require("libs/libkoreader-lfs")
        ffiutil = require("ffi/util")
        G_reader_settings:saveSetting("document_metadata_folder", "doc")

        dir = os.tmpname()
        os.remove(dir)
        lfs.mkdir(dir)
        a_file = touch(dir .. "/alpha.epub")
        b_file = touch(dir .. "/beta.epub")
        c_file = touch(dir .. "/gamma.epub")
        d_file = touch(dir .. "/delta.epub")
        e_file = touch(dir .. "/broken.epub")
        f_file = touch(dir .. "/empty.epub")
        gone_file = dir .. "/gone.epub"

        writeSidecar(a_file, {
            highlight("First quoted line of the book.", "2026-03-01 10:00:00"),
            highlight("A second passage with a note.", "2026-03-02 11:00:00", "check this claim"),
            bookmark("2026-03-03 12:00:00", "Chapter 3"),
        }, { title = "Alpha Book", authors = "Ann Author" })
        writeSidecar(b_file, {
            highlight("Newest highlight.", "2026-05-01 09:00:00"),
        })
        -- c has no sidecar at all.
        writeSidecar(d_file, {
            highlight("Hidden by history.", "2026-06-01 09:00:00"),
        })
        writeSidecar(f_file, {})
        -- A sidecar that does not parse.
        writeSidecar(e_file, { highlight("Broken.", "2026-07-01 09:00:00") })
        local broken_path = sidecarPath(e_file)
        local bf = assert(io.open(broken_path, "w"))
        bf:write("return { annotations = {")
        bf:close()
    end)

    teardown(function()
        for _, file in ipairs(created) do
            removeBookFiles(file)
        end
        lfs.rmdir(dir)
    end)

    describe("NotesData.books", function()
        local function history(...)
            local items = {}
            for i, file in ipairs({ ... }) do
                items[i] = { file = file }
            end
            return items
        end

        it("returns an empty list for an empty history", function()
            assert.are.same({}, NotesData.books({}))
        end)

        it("skips books without a sidecar, without annotations, dimmed, missing or unreadable", function()
            local books = NotesData.books({
                { file = c_file },
                { file = f_file },
                { file = d_file, dim = true },
                { file = gone_file },
                { file = e_file },
            })
            assert.are.same({}, books)
        end)

        it("keeps the broken sidecar on disk and does not write to it", function()
            local before = readAll(sidecarPath(e_file))
            NotesData.books(history(e_file, a_file))
            assert.are.equal(before, readAll(sidecarPath(e_file)))
            assert.is_truthy(lfs.attributes(sidecarPath(e_file), "mode"))
        end)

        it("counts highlights, notes and bookmarks", function()
            local books = NotesData.books(history(a_file))
            assert.are.equal(1, #books)
            local book = books[1]
            assert.are.equal(a_file, book.file)
            assert.are.equal(1, book.count_highlights)
            assert.are.equal(1, book.count_notes)
            assert.are.equal(1, book.count_bookmarks)
            assert.are.equal("2026-03-03 12:00:00", book.last_datetime)
        end)

        it("reads title and authors from doc_props and falls back to the file name", function()
            local books = NotesData.books(history(a_file, b_file))
            local by_file = {}
            for _, book in ipairs(books) do
                by_file[book.file] = book
            end
            assert.are.equal("Alpha Book", by_file[a_file].title)
            assert.are.equal("Ann Author", by_file[a_file].authors)
            assert.are.equal("beta", by_file[b_file].title)
            assert.is_nil(by_file[b_file].authors)
        end)

        it("sorts newest first and keeps history order for equal dates", function()
            local books = NotesData.books(history(a_file, b_file))
            assert.are.equal(2, #books)
            assert.are.equal(b_file, books[1].file)
            assert.are.equal(a_file, books[2].file)
        end)

        it("keeps history order when two books share a date", function()
            local twin_a = touch(dir .. "/twin-a.epub")
            local twin_b = touch(dir .. "/twin-b.epub")
            writeSidecar(twin_a, { highlight("Same day.", "2026-04-04 04:04:04") })
            writeSidecar(twin_b, { highlight("Same day too.", "2026-04-04 04:04:04") })
            local books = NotesData.books(history(twin_b, twin_a))
            assert.are.equal(twin_b, books[1].file)
            assert.are.equal(twin_a, books[2].file)
        end)

        it("does not change the sidecar it reads", function()
            local before = readAll(sidecarPath(a_file))
            NotesData.books(history(a_file))
            assert.are.equal(before, readAll(sidecarPath(a_file)))
        end)
    end)

    describe("NotesData.entries", function()
        it("returns no entries for a book without a sidecar", function()
            assert.are.same({}, NotesData.entries(c_file))
        end)

        it("returns entries in the order of the annotations array", function()
            local entries = NotesData.entries(a_file)
            assert.are.equal(3, #entries)
            assert.are.equal("highlight", entries[1].kind)
            assert.are.equal("note", entries[2].kind)
            assert.are.equal("bookmark", entries[3].kind)
        end)

        it("maps the fields of each annotation", function()
            local entries = NotesData.entries(a_file)
            assert.are.equal("First quoted line of the book.", entries[1].text)
            assert.is_nil(entries[1].note)
            assert.are.equal("Chapter 2", entries[1].chapter)
            assert.are.equal("2026-03-01 10:00:00", entries[1].datetime)
            assert.are.equal("/body/DocFragment[3]/body/p[4].0", entries[1].location.page)
            assert.are.equal("/body/DocFragment[3]/body/p[4].0", entries[1].location.pos0)

            assert.are.equal("check this claim", entries[2].note)
            assert.are.equal("A second passage with a note.", entries[2].text)

            assert.is_nil(entries[3].text)
            assert.is_nil(entries[3].location.pos0)
            assert.are.equal("Chapter 3", entries[3].chapter)
        end)

        it("does not change the sidecar it reads", function()
            local before = readAll(sidecarPath(a_file))
            NotesData.entries(a_file)
            assert.are.equal(before, readAll(sidecarPath(a_file)))
        end)
    end)

    describe("sticker extension point", function()
        after_each(function()
            NotesData.stickerProvider = nil
        end)

        it("defaults to no provider", function()
            assert.is_nil(NotesData.stickerProvider)
            assert.is_nil(NotesData.stickers())
        end)

        it("returns the provider result", function()
            NotesData.stickerProvider = function()
                return { by_file = { [a_file] = {} }, orphaned = { { kind = "sticker", file = b_file } } }
            end
            local result = NotesData.stickers()
            assert.are.equal(1, #result.orphaned)
        end)

        it("returns nil when the provider fails", function()
            NotesData.stickerProvider = function() error("boom") end
            assert.is_nil(NotesData.stickers())
        end)
    end)

    describe("Notes widget", function()
        it("builds with an empty history", function()
            local notes = Notes:new{ history = {} }
            paint(notes)
            assert.are.equal(0, #notes.books)
            -- Nav bar row only.
            assert.are.equal(1, #notes.layout)
            notes:onClose()
        end)

        it("builds with books and opens a book's entries", function()
            local history = { { file = b_file }, { file = a_file }, { file = c_file } }
            local notes = Notes:new{ history = history }
            paint(notes)
            assert.are.equal(2, #notes.books)

            notes:showBook(notes.books[2])
            paint(notes)
            assert.are.equal(a_file, notes.book.file)
            assert.are.equal(3, #notes.entries)
            -- Three entry rows and the nav bar row. The back action is in the header.
            assert.are.equal(4, #notes.layout)

            -- Back in the entries view goes to the book list, not out of Notes.
            notes:onClose()
            assert.is_nil(notes.book)
            paint(notes)
            notes:onClose()
        end)

        it("pages a long book list", function()
            local history = {}
            for i = 1, 60 do
                local file = touch(dir .. "/many-" .. i .. ".epub")
                writeSidecar(file, { highlight("Passage " .. i .. ".", "2026-01-01 00:00:" .. string.format("%02d", i % 60)) })
                history[#history + 1] = { file = file }
            end
            local notes = Notes:new{ history = history }
            paint(notes)
            assert.are.equal(60, #notes.books)
            assert.are.equal(1, notes.page)
            notes:turnPage(1)
            paint(notes)
            assert.are.equal(2, notes.page)
            notes:onClose()
        end)

        it("shows orphaned stickers under a repair group when a provider is set", function()
            NotesData.stickerProvider = function()
                return {
                    by_file = {},
                    orphaned = {
                        { kind = "sticker", text = "Lost sticker", file = a_file, location = { page = "/x" } },
                    },
                }
            end
            local notes = Notes:new{ history = { { file = a_file } } }
            paint(notes)
            -- Repair row, book row, nav bar row.
            assert.are.equal(3, #notes.layout)
            notes:onClose()
            NotesData.stickerProvider = nil
        end)
    end)
end)
