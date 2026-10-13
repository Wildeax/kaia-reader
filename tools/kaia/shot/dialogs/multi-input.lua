-- Screenshot action for the dialog restyle check (KR-013). Shows a
-- MultiInputDialog with three fields over the Library.
local MultiInputDialog = require("ui/widget/multiinputdialog")
local UIManager = require("ui/uimanager")
local _ = require("gettext")

require("kaia/home").closeIfShown()
UIManager:show(MultiInputDialog:new{
    title = _("Edit book details"),
    fields = {
        { description = _("Title"), text = "The Left Hand of Darkness", hint = _("Title") },
        { description = _("Author"), text = "Ursula K. Le Guin", hint = _("Author") },
        { description = _("Series"), text = "", hint = _("Series") },
    },
    buttons = {
        {
            { text = _("Cancel"), callback = function() end },
            { text = _("Save"), is_enter_default = true, callback = function() end },
        },
    },
})
