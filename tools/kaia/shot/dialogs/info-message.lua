-- Screenshot action for the dialog restyle check (KR-013). Shows an InfoMessage
-- over the Library.
local InfoMessage = require("ui/widget/infomessage")
local UIManager = require("ui/uimanager")
local _ = require("gettext")

require("kaia/home").closeIfShown()
UIManager:show(InfoMessage:new{
    text = _("Export finished. 12 notes were written to the notes folder."),
})
