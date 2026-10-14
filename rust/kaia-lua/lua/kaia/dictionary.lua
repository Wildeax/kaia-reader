-- Plugin buttons use the native dictionary's current word and result text.
return function(ui)
    local buttons = {}
    local popup
    local dictionary = {}
    function dictionary:addToDictButtons(button)
        buttons[button.id] = { button = button, owner = KAIA_PLUGIN.loading_name or KAIA_PLUGIN.owner }
    end
    function dictionary:onLookupWord(word)
        KAIA_DEVICE("lookup", word)
    end
    function dictionary:lookup(word) self:onLookupWord(word) end
    function dictionary:setLookup(word, text, context)
        ui.highlight.selected_text = context and context.selected_text or nil
        ui.highlight.lookup_context = context
        popup = {
            ui = ui, word = word, lookupword = word, selected_link = nil,
            window_list = {}, dict_index = 1, results = {{ word = word, definition = text, dict = "Kaia dictionary" }},
            button_table = { button_by_id = {} },
            onClose = function() KAIA_DEVICE("dictionary_close", true) end,
        }
        popup.window_list[1] = popup
        for id, entry in pairs(buttons) do
            popup.button_table.button_by_id[id] = {
                width = 0, dimen = require("ui/geometry"):new{},
                setText = function(_, text) entry.button.text = text end,
            }
        end
        KAIA_PLUGIN.dispatch("WordLookedUp", word, ui.doc_props.display_title)
    end
    function dictionary:showActions()
        if not popup then return end
        local items = {}
        for _, entry in pairs(buttons) do
            local button, owner = entry.button, entry.owner
            local visible = true
            if button.show_func then
                local ok, result = KAIA_PLUGIN.guarded(owner, button.show_func, popup)
                visible = ok and result
            end
            if visible then
                items[#items + 1] = {
                    text = button.text or button.menu_text,
                    callback = function() KAIA_PLUGIN.guarded(owner, button.callback, popup) end,
                    hold_callback = button.hold_callback and function() KAIA_PLUGIN.guarded(owner, button.hold_callback, popup) end,
                }
            end
        end
        require("ui/uimanager"):show(require("kaia/plugin_menu"):new{
            title = require("gettext")("Dictionary plugins"), item_table = items,
        })
    end
    return dictionary
end
