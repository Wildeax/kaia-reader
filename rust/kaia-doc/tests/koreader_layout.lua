-- Headless KOReader reference. Run through tools/kaia/cre_compare.py.
-- Only read the emulator tree; all output goes to the supplied directory.
require("setupkoenv")
local cre = require("libs/libkoreader-cre")
local ffi = require("ffi")
local BB = require("ffi/blitbuffer")
local json = require("rapidjson")
local function read(path)
    local file = assert(io.open(path, "rb"))
    local data = file:read("*a")
    file:close()
    return data
end
local function write(path, data)
    local file = assert(io.open(path, "wb"))
    assert(file:write(data))
    assert(file:close())
end
local book, settings_path, output = unpack(arg)
local s = json.decode(read(settings_path))
cre.initHyphDict("./data/hyph/")
for _, file in ipairs(s.font_files) do
    cre.registerFont(file)
end
cre.regularizeRegisteredFontsWeights(false)
local found = false
for _, face in ipairs(cre.getFontFaces()) do
    if face == s.font_family then found = true end
end
assert(found, "requested font is unavailable")
cre.setAsPreferredFontWithBias(s.font_family, 1 + 128 * 5 + 256 * 5)
local doc = cre.newDocView(s.width, s.height, 1)
doc:setHeaderInfo(0)
doc:setIntProperty("font.hinting.mode", 2)
doc:setIntProperty("font.kerning.mode", 3)
assert(doc:loadDocument(book, false))
doc:setStyleSheet("./data/epub.css", s.css_extra)
doc:setFontFace(s.font_family)
doc:setFontSize(s.font_size)
doc:setIntProperty("font.face.base.weight", s.font_weight)
doc:setPageMargins(s.margin_left, s.margin_top, s.margin_right, s.margin_bottom)
doc:setDefaultInterlineSpace(s.line_percent)
doc:setStringProperty("crengine.hyphenation.directory", s.hyphenation)
doc:setIntProperty("crengine.doc.embedded.styles.enabled", s.embedded_styles and 1 or 0)
doc:setIntProperty("crengine.doc.embedded.fonts.enabled", 1)
doc:renderDocument()
local count = doc:getPages()
assert(count >= 2, "comparison corpus requires at least two pages")
local samples = {0, 1, 2, math.floor(count / 4), math.floor(count / 2), math.floor(count * 3 / 4), count - 2}
table.sort(samples)
local pages, previous = {}, -1
for _, page in ipairs(samples) do
    if page ~= previous and page + 1 < count then
        previous = page
        local start = doc:getPageXPointer(page + 1)
        local finish = doc:getPageXPointer(page + 2)
        local text = assert(doc:getTextFromXPointers(start, finish, false))
        doc:gotoPage(page + 1)
        local bb = BB.new(s.width, s.height, BB.TYPE_BB8)
        doc:drawCurrentPage(bb, false, false, false, false)
        local prefix = output .. "/page-" .. page
        bb:writePNG(prefix .. ".png")
        assert(bb.stride == s.width)
        write(prefix .. ".gray", ffi.string(bb.data, s.width * s.height))
        bb:free()
        pages[#pages + 1] = {page = page, start = start, ["end"] = finish, text = text}
    end
end
write(output .. "/snapshot.json", json.encode({page_count = count, pages = pages, settings = s}))
doc:close()
