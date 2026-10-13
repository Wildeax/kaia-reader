--[[--
Kaia Library: the file manager as Kaia's Library surface.

The file manager gets the Kaia nav bar under its file chooser. The chooser is
shorter by the bar height. This module builds the bar and sizes the chooser.
It also sets the cover mosaic as the default display mode, once.

Upstream calls (frontend/apps/filemanager/filemanager.lua, setupLayout):
- `Library.applyDefaultDisplayMode(self)` before the file chooser is built.
- `Library.chooserHeight()` as the chooser `height`.
- `Library.attachNavBar(self, fm_ui)` to wrap the chooser with the bar.

The bar is on when the setting `kaia_library_navbar` is not false. It is on by
default.

@module kaia.library
]]

local Device = require("device")
local NavBar = require("kaia/widget/navbar")
local Theme = require("kaia/theme")
local VerticalGroup = require("ui/widget/verticalgroup")
local logger = require("logger")
local _ = require("gettext")

local Library = {}

-- G_reader_settings key for the nav bar. Missing or true means on.
Library.NAVBAR_SETTING = "kaia_library_navbar"
-- Display mode for new installs: mosaic with cover images.
Library.DEFAULT_MODE = "mosaic_image"
-- Set once the default has been decided, so it never runs again.
Library.DEFAULT_DONE_SETTING = "kaia_library_default_mode_done"

--- True when the nav bar is on.
function Library.navbarEnabled()
    return G_reader_settings:nilOrTrue(Library.NAVBAR_SETTING)
end

--- Height of the nav bar in pixels, or 0 when it is off.
function Library.navHeight()
    if not Library.navbarEnabled() then
        return 0
    end
    -- The bar is a divider line over a row of items. See widget/navbar.lua.
    return Theme.nav_height + Theme.line.divider
end

--- Height for the file chooser, or nil to keep the full screen height.
function Library.chooserHeight()
    if not Library.navbarEnabled() then
        return nil
    end
    return Device.screen:getHeight() - Library.navHeight()
end

-- The coverbrowser plugin writes these values the first time it runs on a
-- device, when neither file manager nor history has a mode yet.
local function isPluginFirstRunDefault(bookinfo)
    return G_reader_settings:isTrue("coverbrowser_initial_default_setup_done")
        and bookinfo:getSetting("filemanager_display_mode") == "list_image_meta"
        and bookinfo:getSetting("history_display_mode") == "mosaic_image"
        and bookinfo:getSetting("collection_display_mode") == "mosaic_image"
end

--- Sets the cover mosaic as the file manager display mode, once.
-- Applies when the user has not chosen a mode. The coverbrowser plugin's own
-- first-run defaults count as not chosen. Does nothing without the plugin.
-- @param filemanager FileManager instance, before its file chooser is built
function Library.applyDefaultDisplayMode(filemanager)
    if G_reader_settings:isTrue(Library.DEFAULT_DONE_SETTING) then
        return false
    end
    -- Without the plugin there is no mode to set. Ask again on the next start.
    local coverbrowser = filemanager and filemanager.coverbrowser
    local bookinfo = package.loaded["bookinfomanager"]
    if not coverbrowser or not bookinfo or not coverbrowser.setDisplayMode then
        return false
    end
    G_reader_settings:makeTrue(Library.DEFAULT_DONE_SETTING)
    local mode = bookinfo:getSetting("filemanager_display_mode")
    if mode ~= nil and not isPluginFirstRunDefault(bookinfo) then
        return false
    end
    logger.info("kaia library: default display mode set to", Library.DEFAULT_MODE)
    coverbrowser:setDisplayMode(Library.DEFAULT_MODE)
    return true
end

--- Adds the nav bar under the file chooser.
-- @param filemanager FileManager instance
-- @param content widget that holds the file chooser
-- @treturn table widget to show: `content` alone when the bar is off
function Library.attachNavBar(filemanager, content)
    if not Library.navbarEnabled() then
        return content
    end
    -- The path stays as the subtitle. The title matches the nav bar label.
    if filemanager.title_bar then
        filemanager.title_bar:setTitle(_("Library"), true)
    end
    local navbar = NavBar:new{
        width = Device.screen:getWidth(),
        active = "library",
        callbacks = {
            home = function() require("kaia/home"):show() end,
            -- Already here.
            library = function() end,
            notes = function() require("kaia/notes"):show() end,
            settings = function()
                if filemanager.menu then
                    filemanager.menu:onShowMenu()
                end
            end,
        },
    }
    return VerticalGroup:new{
        align = "left",
        content,
        navbar,
    }
end

return Library
