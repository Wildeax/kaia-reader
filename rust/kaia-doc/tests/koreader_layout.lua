-- Run from a stock KOReader package with /book/juliet.epub mounted.
-- This compares the raw constructor with kaia-doc's explicit pixel layout.
require("setupkoenv")
local cre = require("libs/libkoreader-cre")
cre.initHyphDict("./data/hyph/")
local files = io.popen("find ./fonts -type f \\( -name '*.ttf' -o -name '*.otf' -o -name '*.ttc' \\)")
for file in files:lines() do
    if not file:find("/urw/") and not file:find("/nerdfonts/symbols.ttf") then
        cre.registerFont(file)
    end
end
files:close()
cre.regularizeRegisteredFontsWeights(false)
cre.setAsPreferredFontWithBias("Noto Serif", 1 + 128 * 5 + 256 * 5)
local doc = cre.newDocView(1264, 1680, 1)
assert(doc:loadDocument("/book/juliet.epub", false))
doc:setStyleSheet("./data/epub.css", nil)
local function pages(label)
    doc:renderDocument()
    print(label, doc:getPages(), doc:getFontFace(), doc:getFontSize())
end
pages("constructor defaults")
doc:setFontFace("Noto Serif")
pages("Noto Serif")
doc:setHeaderInfo(0)
pages("no engine header")
doc:setFontSize(24)
doc:setDefaultInterlineSpace(100)
doc:setIntProperty("font.hinting.mode", 2)
doc:setIntProperty("font.kerning.mode", 3)
doc:setStringProperty("crengine.hyphenation.directory", "@none")
pages("kaia-doc pixel layout")
assert(doc:getPages() == 114)
-- KOReader's UI scales its 22-unit font and margins by min(width,height)/600.
-- The footer and other frontend options are outside this isolated comparison.
doc:setFontSize(math.ceil(22 * 1264 / 600))
doc:setPageMargins(22, 32, 22, 32)
doc:setStringProperty("crengine.hyphenation.directory", "English_US.pattern")
pages("frontend font and margins, without footer")
