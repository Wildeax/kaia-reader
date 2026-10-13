-- Screenshot action for the dialog restyle check (KR-013). Opens the file
-- manager main menu (TouchMenu) over the Library.
local FileManager = require("apps/filemanager/filemanager")

require("kaia/home").closeIfShown()
if FileManager.instance and FileManager.instance.menu then
    FileManager.instance.menu:onShowMenu()
end
