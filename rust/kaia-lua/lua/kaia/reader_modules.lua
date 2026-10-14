-- Reader methods that translate KOReader annotation tables to host writes.
return function(ui, reader, strict)
    local annotation = ui.annotation
    function annotation:addItem(item)
        local id = reader.add_annotation(item)
        self.annotations = reader.annotations()
        for index, saved in ipairs(self.annotations) do
            if saved.kaia_id == id then return index end
        end
        error("ReaderHost did not return the added annotation")
    end
    function annotation:replaceItems(items)
        reader.replace_annotations(items)
        self.annotations = reader.annotations()
    end
    function annotation:getItemIndex(item)
        for index, saved in ipairs(self.annotations) do
            if saved == item or (item.kaia_id and saved.kaia_id == item.kaia_id) then return index end
        end
    end
    function annotation:getNumberOfAnnotations() return #reader.annotations() end
    function annotation:updatePageNumbers()
        self.annotations = reader.annotations()
        for _, item in ipairs(self.annotations) do
            item.pageno = type(item.page) == "string" and reader.page_for_xpointer(item.page) or item.page
        end
    end
    function annotation:onAnnotationsModified(items)
        for _, item in ipairs(items) do reader.update_annotation(item.kaia_id, item) end
        self.annotations = reader.annotations()
    end
    function ui.bookmark:removeItemByIndex(index)
        reader.delete_annotation(assert(annotation.annotations[index].kaia_id))
        annotation.annotations = reader.annotations()
    end
    function ui.bookmark:removeItem(item, index)
        self:removeItemByIndex(index or annotation:getItemIndex(item))
    end
    function ui.bookmark:setBookmarkNote(item, _, note)
        if type(item) == "number" then item = annotation.annotations[item] end
        item.note = note
        reader.update_annotation(item.kaia_id, item)
        annotation.annotations = reader.annotations()
    end
    function ui.bookmark:deleteItemNote(item) self:setBookmarkNote(item, false, nil) end
    function ui.bookmark:getBookmarkPageNumber(item)
        return type(item.page) == "string" and reader.page_for_xpointer(item.page) or item.page
    end
    function ui.highlight:deleteHighlight(index) ui.bookmark:removeItemByIndex(index) end
    function ui.highlight:getSelectedWordContext(nb_words)
        local context = rawget(self, "lookup_context")
        if not context or not self.selected_text then return end
        local before, after = context.before or "", context.after or ""
        local count = math.max(0, math.floor(nb_words or 0)) + 1
        local starts = {}
        for start in before:gmatch("()%S+%s*") do starts[#starts + 1] = start end
        if #starts > count then before = before:sub(starts[#starts - count + 1]) end
        local n = 0
        for ending in after:gmatch("%s*%S+()") do
            n = n + 1
            if n == count then after = after:sub(1, ending - 1); break end
        end
        return before, after
    end
    function ui.document:getSelectedWordContext(_, nb_words)
        return ui.highlight:getSelectedWordContext(nb_words)
    end
    -- No active selection exists until the host supplies one.
    setmetatable(ui.highlight, { __index = function(_, key)
        if key == "selected_text" or key == "onTap" or key == "onHold" then return nil end
        error("highlight." .. tostring(key) .. " is unavailable", 2)
    end })
    function ui.toc:getTocTitleByPage(page)
        local title = ""
        for _, entry in ipairs(reader.toc()) do
            if entry.page and entry.page <= page then title = entry.title end
        end
        return title
    end
    function ui.toc:getChapterPagesLeft(page)
        for _, entry in ipairs(reader.toc()) do
            if entry.page and entry.page > page then return entry.page - page - 1 end
        end
        return reader.page_count() - page
    end
    ui.bookinfo = strict("bookinfo", {
        extendProps = function(props) return props end,
        expandString = function(_, text, file, timestamp)
            if not text then return end
            local props = reader.properties()
            file = file or reader.file()
            local patterns = {
                T = props.display_title or props.title, A = props.authors, S = props.series,
                t = reader.page_count(), c = reader.current_page(),
                p = math.floor(reader.current_page() / math.max(reader.page_count(), 1) * 100 + 0.5),
                C = ui.toc:getTocTitleByPage(reader.current_page()),
                l = ui.toc:getChapterPagesLeft(reader.current_page()),
                F = file, f = file:match("[^/]+$"),
                D = os.date("%Y-%m-%d", timestamp), d = os.date("%m-%d", timestamp),
                m = os.date("%H:%M", timestamp), M = os.date("%H-%M-%S", timestamp),
            }
            return text:gsub("%%(%a)", function(key) return tostring(patterns[key] or "N/A") end)
        end,
    })
    ui.instance = ui
end
