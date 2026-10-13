describe("kaia home", function()
    local BookData, Home, Blitbuffer, Screen, DocumentRegistry, lfs
    local dir
    local a_file, b_file, c_file, missing_file

    local function touch(path)
        local f = assert(io.open(path, "w"))
        f:write("")
        f:close()
        return path
    end

    local function fakeCover()
        return Blitbuffer.new(60, 90, Blitbuffer.TYPE_BB8)
    end

    local function paint(widget)
        local bb = Blitbuffer.new(Screen:getWidth(), Screen:getHeight(), Blitbuffer.TYPE_BB8)
        widget:paintTo(bb, 0, 0)
        bb:free()
    end

    setup(function()
        require("commonrequire")
        disable_plugins()
        require("document/canvascontext"):init(require("device"))
        BookData = require("kaia/bookdata")
        Home = require("kaia/home")
        Blitbuffer = require("ffi/blitbuffer")
        Screen = require("device").screen
        DocumentRegistry = require("document/documentregistry")
        lfs = require("libs/libkoreader-lfs")

        dir = os.tmpname()
        os.remove(dir)
        lfs.mkdir(dir)
        a_file = touch(dir .. "/alpha-book.epub")
        b_file = touch(dir .. "/beta.epub")
        c_file = touch(dir .. "/gamma.pdf")
        missing_file = dir .. "/deleted.epub"
    end)

    teardown(function()
        for _, f in ipairs({ a_file, b_file, c_file }) do
            os.remove(f)
        end
        lfs.rmdir(dir)
    end)

    describe("BookData", function()
        local metadata_by_file

        before_each(function()
            metadata_by_file = {}
        end)

        local function provider(file)
            return metadata_by_file[file] or {}
        end

        it("skips files that are missing or marked deleted", function()
            local history = {
                { file = a_file },
                { file = missing_file },
                { file = b_file, dim = true },
                { file = c_file },
            }
            local books = BookData.recent(10, { history = history, metadata = provider })
            assert.are.equal(2, #books)
            assert.are.equal(a_file, books[1].file)
            assert.are.equal(c_file, books[2].file)
        end)

        it("keeps history order and stops at n", function()
            local history = { { file = c_file }, { file = a_file }, { file = b_file } }
            local books = BookData.recent(2, { history = history, metadata = provider })
            assert.are.equal(2, #books)
            assert.are.equal(c_file, books[1].file)
            assert.are.equal(a_file, books[2].file)
        end)

        it("falls back to the file name without extension", function()
            local books = BookData.recent(1, { history = { { file = a_file } }, metadata = provider })
            assert.are.equal("alpha-book", books[1].title)
            assert.is_nil(books[1].authors)
            assert.is_nil(books[1].percent)
            assert.is_nil(books[1].cover_bb)
        end)

        it("uses the title from metadata when present", function()
            metadata_by_file[a_file] = { title = "The Left Hand of Darkness", authors = "Ursula K. Le Guin" }
            local books = BookData.recent(1, { history = { { file = a_file } }, metadata = provider })
            assert.are.equal("The Left Hand of Darkness", books[1].title)
            assert.are.equal("Ursula K. Le Guin", books[1].authors)
        end)

        it("passes percent, status and cover through", function()
            local cover = fakeCover()
            metadata_by_file[b_file] = { percent = 0.42, status = "reading", cover_bb = cover }
            local books = BookData.recent(1, { history = { { file = b_file } }, metadata = provider })
            assert.are.equal(0.42, books[1].percent)
            assert.are.equal("reading", books[1].status)
            assert.are.equal(cover, books[1].cover_bb)
        end)

        it("ignores a percent that is not a number", function()
            metadata_by_file[b_file] = { percent = "half" }
            local books = BookData.recent(1, { history = { { file = b_file } }, metadata = provider })
            assert.is_nil(books[1].percent)
        end)

        it("returns no books for an empty history", function()
            assert.are.same({}, BookData.recent(5, { history = {}, metadata = provider }))
        end)

        it("counts history entries that exist", function()
            local history = { { file = a_file }, { file = missing_file }, { file = b_file, dim = true } }
            assert.are.same({ books_in_history = 1 }, BookData.counts({ history = history }))
        end)

        it("reads metadata without opening the document", function()
            local opened = 0
            local real_open = DocumentRegistry.openDocument
            DocumentRegistry.openDocument = function(...)
                opened = opened + 1
                return real_open(...)
            end
            local ok, books = pcall(BookData.recent, 2, { history = { { file = a_file }, { file = c_file } } })
            DocumentRegistry.openDocument = real_open
            assert.is_true(ok)
            assert.are.equal(2, #books)
            assert.are.equal(0, opened)
        end)
    end)

    describe("Home", function()
        local history_files

        before_each(function()
            history_files = {
                { file = a_file },
                { file = b_file },
                { file = c_file },
            }
        end)

        it("builds with an empty history", function()
            local home = Home:new{
                history = {},
                metadata = function() return {} end,
            }
            paint(home)
            -- Button row and nav bar row.
            assert.are.equal(2, #home.layout)
            home:refresh()
            paint(home)
            home:onClose()
        end)

        it("builds with a non-empty history", function()
            local home = Home:new{
                history = history_files,
                metadata = function(file)
                    if file == a_file then
                        return { percent = 0.42, title = "Alpha", authors = "A. Author", cover_bb = fakeCover() }
                    end
                    return {}
                end,
            }
            paint(home)
            -- Hero row, shelf row and nav bar row.
            assert.are.equal(3, #home.layout)
            assert.are.equal(1, #home.layout[1])
            assert.are.equal(2, #home.layout[2])
            home:refresh()
            paint(home)
            home:onClose()
        end)

        it("closeIfShown closes the shown Home and does nothing when none is shown", function()
            Home.closeIfShown()
            local closed = {}
            local UIManager = require("ui/uimanager")
            local orig_close = UIManager.close
            UIManager.close = function(self, widget, ...)
                closed[#closed + 1] = widget
                return orig_close(self, widget, ...)
            end
            local ok, err = pcall(function()
                local home = Home:show()
                Home.closeIfShown()
                assert.are.equal(home, closed[#closed])
                local count = #closed
                Home.closeIfShown()
                assert.are.equal(count, #closed)
            end)
            UIManager.close = orig_close
            assert.is_true(ok, err)
        end)

        it("builds with more books than the shelf shows", function()
            local history = {}
            for i = 1, 8 do
                history[i] = { file = (i % 2 == 0) and a_file or b_file }
            end
            local home = Home:new{
                history = history,
                metadata = function() return {} end,
            }
            paint(home)
            -- The shelf shows at most four books after the hero.
            assert.are.equal(4, #home.layout[2])
            home:onClose()
        end)
    end)
end)
