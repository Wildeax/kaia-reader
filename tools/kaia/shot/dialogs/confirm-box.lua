-- Screenshot action for the dialog restyle check (KR-013). Shows a ConfirmBox
-- over the Library. Run through tools/kaia/screenshot.sh.
local ConfirmBox = require("ui/widget/confirmbox")
local UIManager = require("ui/uimanager")
local _ = require("gettext")

require("kaia/home").closeIfShown()
UIManager:show(ConfirmBox:new{
    text = _("Remove this book from the library? The file on the device is kept."),
    ok_text = _("Remove"),
    cancel_text = _("Cancel"),
    ok_callback = function() end,
})
