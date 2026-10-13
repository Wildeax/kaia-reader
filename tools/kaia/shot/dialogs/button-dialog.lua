-- Screenshot action for the dialog restyle check (KR-013). Shows a ButtonDialog
-- with four buttons over the Library.
local ButtonDialog = require("ui/widget/buttondialog")
local UIManager = require("ui/uimanager")
local _ = require("gettext")

local function noop() end

require("kaia/home").closeIfShown()
UIManager:show(ButtonDialog:new{
    title = _("Sort books"),
    buttons = {
        {
            { text = _("Title"), callback = noop },
            { text = _("Author"), callback = noop },
        },
        {
            { text = _("Date added"), callback = noop },
            { text = _("Last read"), callback = noop },
        },
    },
})
