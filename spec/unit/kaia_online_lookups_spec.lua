--[[--
Tests for frontend/kaia/online_lookups.lua and the guards that use it.

The Translate and Wikipedia actions send selected text over the network.
These tests check that both switches are off by default, that the popup and
the long-press menus hide the two actions while they are off, and that the
request paths make no HTTP request while they are off. The HTTP layer is
replaced by a stub that counts calls, so the tests stay offline.
]]

describe("Kaia online lookups", function()
    local OnlineLookups, Translator, ReaderHighlight, ReaderWikipedia, UIManager
    local shown, http_calls, real_http, real_show

    setup(function()
        require("commonrequire")
        OnlineLookups = require("kaia/online_lookups")
        Translator = require("ui/translator")
        ReaderHighlight = require("apps/reader/modules/readerhighlight")
        ReaderWikipedia = require("apps/reader/modules/readerwikipedia")
        UIManager = require("ui/uimanager")
    end)

    -- Fake reader UI: enough for the widgets under test. A LookupWikipedia
    -- event goes to the real ReaderWikipedia module, as in the reader.
    local function newUI()
        local ui = {
            registerPostInitCallback = function() end,
            menu = { registerToMainMenu = function() end },
        }
        ui.handleEvent = function(_, ev)
            if ev.handler == "onLookupWikipedia" and ui.wikipedia then
                return ui.wikipedia:onLookupWikipedia(table.unpack(ev.args, 1, ev.args.n))
            end
        end
        return ui
    end

    local function newHighlight()
        local ui = newUI()
        local hl = ReaderHighlight:new{
            ui = ui,
            document = { info = { has_pages = true } },
            view = { highlight = { saved_drawer = "lighten", saved_color = "yellow" } },
        }
        ui.wikipedia = ReaderWikipedia:new{ ui = ui }
        hl.selected_text = { text = "hello", pos0 = {}, pos1 = {} }
        return hl
    end

    -- Builds the highlight popup and returns the text of every button in it.
    local function popupTexts(hl)
        shown = {}
        hl:onShowHighlightMenu()
        local dialog = shown[#shown]
        assert.truthy(dialog and dialog.buttons)
        local texts = {}
        for _, row in ipairs(dialog.buttons) do
            for _, button in ipairs(row) do
                table.insert(texts, button.text)
            end
        end
        return texts
    end

    local function contains(list, value)
        for _, item in ipairs(list) do
            if item == value then return true end
        end
        return false
    end

    before_each(function()
        G_reader_settings:delSetting("kaia_allow_translate")
        G_reader_settings:delSetting("kaia_allow_wikipedia")
        shown = {}
        http_calls = 0
        real_http = package.loaded["socket.http"]
        real_show = UIManager.show
        -- Any request through socket.http counts as a failure.
        package.loaded["socket.http"] = {
            request = function()
                http_calls = http_calls + 1
                return nil, "request stubbed by kaia_online_lookups_spec"
            end,
        }
        UIManager.show = function(_, widget)
            table.insert(shown, widget)
        end
    end)

    after_each(function()
        package.loaded["socket.http"] = real_http
        UIManager.show = real_show
        G_reader_settings:delSetting("kaia_allow_translate")
        G_reader_settings:delSetting("kaia_allow_wikipedia")
    end)

    it("keeps both switches off by default", function()
        assert.is_false(OnlineLookups:isAllowed("translate"))
        assert.is_false(OnlineLookups:isAllowed("wikipedia"))
        assert.is_false(G_reader_settings:isTrue("kaia_allow_translate"))
        assert.is_false(G_reader_settings:isTrue("kaia_allow_wikipedia"))
    end)

    it("hides the Translate and Wikipedia buttons while both are off", function()
        local texts = popupTexts(newHighlight())
        assert.is_true(contains(texts, "Dictionary"))
        assert.is_false(contains(texts, "Translate"))
        assert.is_false(contains(texts, "Wikipedia"))
    end)

    it("hides the Translate and Wikipedia actions from the long-press choices while both are off", function()
        local action_nums, action_texts = newHighlight():getHighlightActions()
        assert.is_true(#action_nums > 0)
        assert.is_true(contains(action_texts, "Dictionary"))
        assert.is_false(contains(action_texts, "Translate"))
        assert.is_false(contains(action_texts, "Wikipedia"))
    end)

    it("makes no request from the translate entry points while translate is off", function()
        assert.is_nil(Translator:loadPage("hello", "fr", "en"))
        assert.is_nil(Translator:translate("hello", "fr", "en"))
        assert.are.equal("en", Translator:detect("hello"))
        Translator:showTranslation("hello", true, "en", "fr")
        newHighlight():translate()
        assert.are.equal(0, #shown)
        assert.are.equal(0, http_calls)
    end)

    it("makes no request from the Wikipedia entry points while Wikipedia is off", function()
        local ui = newUI()
        local wiki = ReaderWikipedia:new{ ui = ui }
        ui.wikipedia = wiki
        wiki:lookupWikipedia("hello", true)
        assert.is_true(wiki:onShowWikipediaLookup())
        newHighlight():lookupWikipedia()
        assert.are.equal(0, #shown)
        assert.are.equal(0, http_calls)
    end)

    it("disables the Wikipedia lookup menu item while Wikipedia is off", function()
        local menu = {}
        ReaderWikipedia:new{ ui = newUI() }:addToMainMenu(menu)
        assert.is_false(menu.wikipedia_lookup.enabled_func())
    end)

    it("asks for confirmation before Translate turns on, then shows the button", function()
        local item = OnlineLookups:menuItem("translate")
        assert.is_false(item.checked_func())
        item.callback()
        local box = shown[#shown]
        assert.are.equal("Selected text will be sent to Google Translate over the internet.", box.text)
        assert.are.equal("Turn on", box.ok_text)
        assert.are.equal("Cancel", box.cancel_text)
        assert.is_false(OnlineLookups:isAllowed("translate"))
        assert.is_false(contains(popupTexts(newHighlight()), "Translate"))

        box.ok_callback()
        assert.is_true(item.checked_func())
        local texts = popupTexts(newHighlight())
        assert.is_true(contains(texts, "Translate"))
        assert.is_false(contains(texts, "Wikipedia"))
    end)

    it("asks for confirmation before Wikipedia turns on, then shows the button", function()
        local item = OnlineLookups:menuItem("wikipedia")
        item.callback()
        local box = shown[#shown]
        assert.are.equal("Selected text will be sent to Wikipedia over the internet.", box.text)
        assert.are.equal("Turn on", box.ok_text)
        assert.are.equal("Cancel", box.cancel_text)
        assert.is_false(OnlineLookups:isAllowed("wikipedia"))

        box.ok_callback()
        assert.is_true(OnlineLookups:isAllowed("wikipedia"))
        local texts = popupTexts(newHighlight())
        assert.is_true(contains(texts, "Wikipedia"))
        assert.is_false(contains(texts, "Translate"))
    end)

    it("leaves the switch off when the user cancels the confirmation", function()
        local item = OnlineLookups:menuItem("translate")
        item.callback()
        shown[#shown].cancel_callback()
        assert.is_false(OnlineLookups:isAllowed("translate"))
        assert.is_false(contains(popupTexts(newHighlight()), "Translate"))
    end)

    it("turns a switch off without asking again", function()
        local item = OnlineLookups:menuItem("translate")
        item.callback()
        shown[#shown].ok_callback()
        shown = {}
        item.callback()
        assert.are.equal(0, #shown)
        assert.is_false(OnlineLookups:isAllowed("translate"))
        assert.is_false(item.checked_func())
    end)
end)
