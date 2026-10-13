-- Screenshot action for KR-010. Shows Kaia Home, then a ConfirmBox created the
-- way plugins create one, on top of Home. Run through tools/kaia/screenshot.sh.
local ConfirmBox = require("ui/widget/confirmbox")
local UIManager = require("ui/uimanager")
local _ = require("gettext")

require("kaia/home"):show()
UIManager:show(ConfirmBox:new{
    text = _("Remove this book from the library? The file on the device is kept."),
    ok_text = _("Remove"),
    cancel_text = _("Cancel"),
    ok_callback = function() end,
})
