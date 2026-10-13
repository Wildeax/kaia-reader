--[[--
Kaia Extensions: the plugins the loader knows about, with their origin,
version, state, compatibility and configuration.

One full screen widget. The list shows every plugin, sorted by name. Tapping a
plugin opens its detail view, which shows
the description, the load error if it failed, and a button that turns the
plugin on or off. The button writes the `plugins_disabled` setting, the same
setting the plugin manager uses, and then asks for a restart.

The plugin list comes from the running PluginLoader. The loader is not asked
to scan again, except to find plugins that did not load at all.

Usage:
    require("kaia/extensions"):show()

Compatibility comes from kaia/compat_profile, which tools/kaia/inventory
generates. Plugins that are not in the profile are shown as best effort.

@module kaia.extensions
]]

local CenterContainer = require("ui/widget/container/centercontainer")
local Device = require("device")
local FocusManager = require("ui/widget/focusmanager")
local FrameContainer = require("ui/widget/container/framecontainer")
local Geom = require("ui/geometry")
local GestureRange = require("ui/gesturerange")
local Header = require("kaia/widget/header")
local Home = require("kaia/home")
local HorizontalGroup = require("ui/widget/horizontalgroup")
local HorizontalSpan = require("ui/widget/horizontalspan")
local InputContainer = require("ui/widget/container/inputcontainer")
local LineWidget = require("ui/widget/linewidget")
local NavBar = require("kaia/widget/navbar")
local PageButton = require("kaia/widget/pagebutton")
local PluginLoader = require("pluginloader")
local Profile = require("kaia/compat_profile")
local SafeMode = require("kaia/safemode")
local TextBoxWidget = require("ui/widget/textboxwidget")
local TextWidget = require("ui/widget/textwidget")
local Theme = require("kaia/theme")
local UIManager = require("ui/uimanager")
local VerticalGroup = require("ui/widget/verticalgroup")
local VerticalSpan = require("ui/widget/verticalspan")
local _ = require("gettext")
local T = require("ffi/util").template

local Screen = Device.screen

-- Bundled plugins live in the default plugin directory. This is DEFAULT_PLUGIN_PATH
-- in frontend/pluginloader.lua. Other plugins come from the data directory.
local BUNDLED_DIR = "plugins"

-- Lines of text per field in the detail view before the text is cut with an ellipsis.
local DETAIL_LINES = { compat = 2, path = 2, description = 4, error = 6, note = 3 }

local Extensions = FocusManager:extend{
    name = "kaia_extensions",
    covers_fullscreen = true,
    -- Test injection. Nil uses the running PluginLoader, the live safe mode state
    -- and the built-in compatibility profile.
    loader = nil,
    safe_mode = nil,
    profile = nil,
    -- The row that the detail view shows, or nil for the list.
    detail = nil,
}

-- The Extensions instance on the widget stack. Only one is shown at a time.
local current

--- Origin text. Bundled plugins come from the app's plugin directory.
function Extensions.isBundled(path)
    return type(path) == "string" and path:sub(1, #BUNDLED_DIR + 1) == BUNDLED_DIR .. "/"
end

--- Short state text for a row state ("on", "off", "failed" or "skipped").
local function stateText(state)
    if state == "on" then
        return _("On")
    elseif state == "failed" then
        return _("Failed to load")
    elseif state == "skipped" then
        return _("Skipped in safe mode")
    end
    return _("Off")
end

--- Compatibility text for a profile entry, or nil when the plugin is not in the profile.
function Extensions.compatibilityText(entry)
    if not entry then
        return _("Not in the compatibility profile; works on a best effort basis")
    end
    if entry.class == "Kaia" then
        return _("Kaia")
    end
    if entry.platform == "not applicable" or entry.platform == "debug only" then
        return _("Not applicable on this device")
    end
    if entry.tested then
        return _("Bundled, device tested")
    end
    return _("Bundled, not yet device tested")
end

local function versionText(version)
    if type(version) == "number" then
        return tostring(version)
    end
    if type(version) == "string" and version ~= "" then
        return version
    end
    return nil
end

--- Whether the user may turn a plugin on. Third-party plugins stay off while the
-- plugins_disable_external setting is on.
local function canTurnOn(bundled, disable_external)
    return bundled or not disable_external
end

-- True when the loader lists the plugin as bundled. Falls back to the path
-- for loaders that do not expose the list (older stand-ins in tests).
local function isBundledPlugin(opts, plugin)
    local builtin = opts.loader and opts.loader.builtin_plugins
    if builtin then
        return builtin[plugin.name] and true or false
    end
    return Extensions.isBundled(plugin.path)
end

-- Builds one row. `plugin` has name, path and optionally fullname, description and version.
-- `state` is the state the caller found, before the error check.
local function makeRow(plugin, state, in_list, opts, load_error)
    local name = plugin.name
    local bundled = isBundledPlugin(opts, plugin)
    local entry = opts.profile.plugins[name]
    if load_error then
        state = "failed"
    end
    local row = {
        name = name,
        fullname = plugin.fullname or name,
        description = plugin.description,
        path = plugin.path,
        bundled = bundled,
        origin = bundled and "bundled" or "user",
        version = versionText(plugin.version),
        state = state,
        error = load_error,
        compat = Extensions.compatibilityText(entry),
        in_list = in_list,
    }
    -- Whether the toggle can act, and the note to show when it cannot.
    row.can_toggle = false
    if state == "on" then
        row.can_toggle = true
    elseif state == "off" and in_list then
        if canTurnOn(bundled, opts.disable_external) then
            row.can_toggle = true
        else
            row.note = _("Third-party plugins are off in settings. Allow them to turn this plugin on.")
        end
    elseif state == "skipped" then
        row.note = _("Safe mode skips third-party plugins. Leave safe mode to load them.")
    elseif state == "failed" then
        row.note = _("Fix the error, then restart Kaia.")
    elseif not in_list then
        row.note = _("Not loaded. The loader kept no reason. The plugin's main.lua may turn it off for this device.")
    end
    return row
end

--- Returns one row per plugin, sorted by name.
-- @table opts `loader` (a PluginLoader or a stand-in with the same fields),
-- `profile` (compat profile), `safe_mode` (boolean), `disable_external` (boolean)
-- @treturn table list of rows
function Extensions.collect(opts)
    local loader = opts.loader
    local enabled, disabled = loader:loadPlugins()
    local load_errors = loader.load_errors or {}
    local safe_mode = opts.safe_mode and true or false
    local rows, seen = {}, {}

    local function add(row)
        if not seen[row.name] then
            seen[row.name] = true
            rows[#rows + 1] = row
        end
    end

    for _i, plugin in ipairs(enabled or {}) do
        add(makeRow(plugin, "on", true, opts, load_errors[plugin.name]))
    end
    for _i, plugin in ipairs(disabled or {}) do
        local bundled = isBundledPlugin(opts, plugin)
        local state = (safe_mode and not bundled) and "skipped" or "off"
        add(makeRow(plugin, state, true, opts, load_errors[plugin.name]))
    end
    -- Plugins that are in neither list did not load. The discovery step lists
    -- directories only. It runs no plugin code.
    for _i, found in ipairs(loader:_discover() or {}) do
        if not seen[found.name] then
            add(makeRow(found, "off", false, opts, load_errors[found.name]))
        end
    end

    table.sort(rows, function(a, b)
        local fa, fb = a.fullname:lower(), b.fullname:lower()
        if fa ~= fb then
            return fa < fb
        end
        return a.name < b.name
    end)
    return rows
end

--- Turns a plugin on or off through the plugins_disabled setting, as the plugin
-- manager does. Turning off also stops a running instance. Asks for a restart.
function Extensions.setEnabled(loader, name, enable)
    local disabled = G_reader_settings:readSetting("plugins_disabled") or {}
    if enable then
        disabled[name] = nil
    else
        disabled[name] = true
        loader:stopPluginInstanceByName(name)
    end
    G_reader_settings:saveSetting("plugins_disabled", disabled)
    UIManager:askForRestart()
end

-- Whether the plugin should be on, after any change made in this session.
local function wantedOn(row)
    if row.wanted ~= nil then
        return row.wanted
    end
    return row.state == "on"
end

-- Text for one list row: the name, then one line with state and origin
-- (bundled plugins say so through their compatibility text), then the
-- compatibility text on its own line only for plugins installed by the user.
local function rowLines(row)
    local secondary = stateText(row.state)
    if row.bundled then
        secondary = secondary .. ". " .. row.compat
        return row.fullname, secondary, nil
    end
    secondary = secondary .. ", " .. _("Installed by you")
    if row.version then
        secondary = secondary .. ", " .. T(_("version %1"), row.version)
    end
    return row.fullname, secondary, row.compat
end

-- Text block capped at `max_lines`. Longer text ends with an ellipsis.
local function boundedText(text, width, max_lines)
    local face = Theme:font("body")
    local line_h = math.floor((1 + 0.3) * face.size + 0.5)
    return TextBoxWidget:new{
        text = text,
        face = face,
        fgcolor = Theme.color.ink,
        width = width,
        height = line_h * max_lines,
        height_adjust = true,
        height_overflow_show_ellipsis = true,
    }
end

-- A label above a value. The value is capped at `max_lines`.
local function field(label, value, width, max_lines)
    return VerticalGroup:new{
        align = "left",
        TextWidget:new{
            text = label,
            face = Theme:font("caption"),
            fgcolor = Theme.color.ink_secondary,
            max_width = width,
        },
        boundedText(value, width, max_lines),
        VerticalSpan:new{ width = Theme.space.md },
    }
end

-- One list row with a primary line, two secondary lines and a hairline divider.
local ExtensionRow = InputContainer:extend{
    width = 0,
    primary = "",
    secondary = "",
    tertiary = "",
    callback = nil,
}

function ExtensionRow:init()
    local text_w = math.max(1, self.width)
    local column = VerticalGroup:new{
        align = "left",
        VerticalSpan:new{ width = Theme.space.md },
        TextWidget:new{
            text = self.primary,
            face = Theme:font("body"),
            fgcolor = Theme.color.ink,
            max_width = text_w,
        },
        VerticalSpan:new{ width = Theme.space.xs },
        TextWidget:new{
            text = self.secondary,
            face = Theme:font("caption"),
            fgcolor = Theme.color.ink_secondary,
            max_width = text_w,
        },
        VerticalSpan:new{ width = Theme.space.md },
        LineWidget:new{
            dimen = Geom:new{ w = self.width, h = Theme.line.divider },
            background = Theme.color.divider,
        },
    }
    if self.tertiary and self.tertiary ~= "" then
        table.insert(column, 5, VerticalSpan:new{ width = Theme.space.xs })
        table.insert(column, 6, TextWidget:new{
            text = self.tertiary,
            face = Theme:font("caption"),
            fgcolor = Theme.color.ink_secondary,
            max_width = text_w,
        })
    end
    self[1] = column
    self.dimen = Geom:new{ x = 0, y = 0, w = self.width, h = column:getSize().h }
    self.ges_events = {
        TapExtensionRow = {
            GestureRange:new{
                ges = "tap",
                range = function() return self.dimen end,
            },
        },
    }
end

function ExtensionRow:onFocus()
    self._focused = true
    return true
end

function ExtensionRow:onUnfocus()
    self._focused = nil
    return true
end

function ExtensionRow:paintTo(bb, x, y)
    InputContainer.paintTo(self, bb, x, y)
    if self._focused then
        bb:paintBorder(x, y, self.dimen.w, self.dimen.h, Theme.line.focus, Theme.color.ink)
    end
end

function ExtensionRow:onTapExtensionRow()
    if self.callback then
        self.callback()
    end
    return true
end

-- Height of the widget that `build` returns.
local function measure(build)
    return build():getSize().h
end

-- Splits items into pages. Each item has a `height` and is kept whole.
local function paginate(items, avail)
    local pages = {}
    local page, used = {}, 0
    for _i, item in ipairs(items) do
        if #page > 0 and used + item.height > avail then
            pages[#pages + 1] = page
            page, used = {}, 0
        end
        page[#page + 1] = item
        used = used + item.height
    end
    if #page > 0 then
        pages[#pages + 1] = page
    end
    return pages
end

-- Closes this widget and the shown Home, if there is one. Home sits under
-- Extensions when Extensions is opened from Home.
local function closeKaiaSurfaces(widget)
    UIManager:close(widget)
    Home.closeIfShown()
end

--- Shows Extensions on top of the current screen. Replaces an open Extensions.
-- @table opts optional; `loader`, `safe_mode` and `profile` are passed to tests.
function Extensions:show(opts)
    if current then
        UIManager:close(current)
        current = nil
    end
    local widget = self:new{
        loader = opts and opts.loader,
        safe_mode = opts and opts.safe_mode,
        profile = opts and opts.profile,
    }
    current = widget
    UIManager:show(widget)
    return widget
end

function Extensions:init()
    self.dimen = Geom:new{ x = 0, y = 0, w = Screen:getWidth(), h = Screen:getHeight() }
    if Device:hasKeys() then
        self.key_events.Close = { { Device.input.group.Back } }
    end
    self.loader = self.loader or PluginLoader
    self.profile = self.profile or Profile
    if self.safe_mode == nil then
        self.safe_mode = SafeMode.active
    end
    self.rows = Extensions.collect{
        loader = self.loader,
        profile = self.profile,
        safe_mode = self.safe_mode,
        disable_external = G_reader_settings:isTrue("plugins_disable_external"),
    }
    self.page = 1
    self:buildContent()
end

--- Rebuilds the screen from the rows and repaints it.
function Extensions:refresh()
    self:buildContent()
    UIManager:setDirty(self, "ui")
end

--- Shows the detail view of one plugin.
function Extensions:showDetail(row)
    self.detail = row
    self.page = 1
    self:refresh()
end

--- Returns from the detail view to the list.
function Extensions:showList()
    self.detail = nil
    self.page = 1
    self:refresh()
end

function Extensions:turnPage(delta)
    self.page = self.page + delta
    self:refresh()
end

--- Goes back one level, or closes Extensions from the list.
function Extensions:back()
    if self.detail then
        self:showList()
    else
        UIManager:close(self)
    end
end

--- Turns the plugin in the detail view on or off, then asks for a restart.
function Extensions:toggleDetail()
    local row = self.detail
    if not row or not row.can_toggle then
        return
    end
    local enable = not wantedOn(row)
    Extensions.setEnabled(self.loader, row.name, enable)
    row.wanted = enable
    self:refresh()
end

-- Rows of the list, each with its height and builder.
function Extensions:listItems(content_w)
    local items = {}
    for _i, row in ipairs(self.rows) do
        local primary, secondary, tertiary = rowLines(row)
        local function build()
            return ExtensionRow:new{
                width = content_w,
                primary = primary,
                secondary = secondary,
                tertiary = tertiary,
                callback = function() self:showDetail(row) end,
            }
        end
        items[#items + 1] = { height = measure(build), build = build, focusable = true }
    end
    return items
end

-- The detail view: one labeled block per fact, then the toggle.
function Extensions:detailBody(row, content_w, gap)
    local body = VerticalGroup:new{ align = "left" }
    local function add(label, value, max_lines)
        table.insert(body, field(label, value, content_w, max_lines))
    end

    local state_value = stateText(row.state)
    if row.wanted ~= nil and row.wanted ~= (row.state == "on") then
        state_value = state_value .. ". " .. _("Restart Kaia to apply the change.")
    end
    add(_("State"), state_value, DETAIL_LINES.note)
    if row.bundled then
        add(_("Origin"), _("Bundled"), 1)
    else
        add(_("Origin"), _("Installed by you"), 1)
    end
    if row.version then
        add(_("Version"), row.version, 1)
    end
    add(_("Compatibility"), row.compat, DETAIL_LINES.compat)
    if row.path then
        add(_("Path"), row.path, DETAIL_LINES.path)
    end
    if row.description and row.description ~= "" then
        add(_("Description"), row.description, DETAIL_LINES.description)
    end
    if row.error then
        add(_("Load error"), row.error, DETAIL_LINES.error)
    end
    if row.note then
        add(_("Note"), row.note, DETAIL_LINES.note)
    end

    local button_text = wantedOn(row) and _("Turn off") or _("Turn on")
    local button = PageButton:new{
        text = button_text,
        width = math.floor(content_w * 0.5),
        enabled = row.can_toggle,
        callback = function() self:toggleDetail() end,
    }
    table.insert(body, VerticalSpan:new{ width = gap })
    table.insert(body, button)
    return body, button
end

-- Splits the list into pages and returns the page and the footer, if any.
function Extensions:listPage(content_w, gap, avail, footer_h)
    local items = self:listItems(content_w)
    if #items == 0 then
        local empty = VerticalGroup:new{ align = "left" }
        table.insert(empty, VerticalSpan:new{ width = gap })
        table.insert(empty, TextBoxWidget:new{
            text = _("No extensions found."),
            face = Theme:font("body"),
            fgcolor = Theme.color.ink,
            width = content_w,
        })
        return empty, {}
    end
    local pages = paginate(items, avail)
    if #pages > 1 then
        pages = paginate(items, avail - footer_h)
    end
    self.page = math.min(math.max(1, self.page), math.max(1, #pages))
    local body = VerticalGroup:new{ align = "left" }
    for _i, item in ipairs(pages[self.page] or {}) do
        local widget = item.build()
        table.insert(body, widget)
        table.insert(self.layout, { widget })
    end
    return body, pages
end

-- The pager row for a paged list, or nil when the list fits on one page.
function Extensions:pager(pages, content_w, gap)
    local btn_w = math.floor(content_w * 0.3)
    local prev = PageButton:new{
        text = _("Previous"),
        width = btn_w,
        enabled = self.page > 1,
        callback = function() self:turnPage(-1) end,
    }
    local next_button = PageButton:new{
        text = _("Next"),
        width = btn_w,
        enabled = self.page < #pages,
        callback = function() self:turnPage(1) end,
    }
    local label_w = content_w - 2 * btn_w - 2 * gap
    local label = TextWidget:new{
        text = T(_("Page %1 of %2"), self.page, #pages),
        face = Theme:font("body"),
        fgcolor = Theme.color.ink,
        max_width = label_w,
    }
    table.insert(self.layout, { prev, next_button })
    return HorizontalGroup:new{
        align = "center",
        prev,
        HorizontalSpan:new{ width = gap },
        CenterContainer:new{
            dimen = Geom:new{ w = label_w, h = Theme.touch_target },
            label,
        },
        HorizontalSpan:new{ width = gap },
        next_button,
    }
end

function Extensions:buildContent()
    local screen_w, screen_h = self.dimen.w, self.dimen.h
    local margin = Theme.space.lg
    local content_w = screen_w - 2 * margin
    local gap = Theme.space.md
    local power = Device:hasBattery() and Device:getPowerDevice():getCapacity() or nil

    -- Focusable rows, top to bottom, then the pager and the nav bar.
    self.selected = { x = 1, y = 1 }
    self.layout = {}

    local navbar = NavBar:new{
        width = screen_w,
        active = "settings",
        callbacks = {
            home = function() self:openHome() end,
            library = function() self:openLibrary() end,
            notes = function() self:openNotes() end,
            settings = function() self:back() end,
        },
    }
    local nav_h = navbar:getSize().h

    local top = VerticalGroup:new{ align = "left" }
    table.insert(top, Header:new{
        title = self.detail and self.detail.fullname or _("Extensions"),
        width = content_w,
        battery_percent = power,
        back_callback = function() self:back() end,
    })
    table.insert(top, VerticalSpan:new{ width = gap })
    if self.safe_mode then
        table.insert(top, FrameContainer:new{
            bordersize = 0,
            padding = gap,
            margin = 0,
            background = Theme.color.surface_alt,
            width = content_w,
            TextBoxWidget:new{
                text = _("Safe mode is on. Third-party extensions are not loaded."),
                face = Theme:font("body"),
                fgcolor = Theme.color.ink,
                width = content_w - 2 * gap,
            },
        })
        table.insert(top, VerticalSpan:new{ width = gap })
    end

    local footer_h = Theme.touch_target + 2 * gap
    local avail = screen_h - gap - top:getSize().h - nav_h
    local body, footer
    if self.detail then
        local block, button = self:detailBody(self.detail, content_w, gap)
        body = block
        table.insert(self.layout, { button })
    else
        local pages
        body, pages = self:listPage(content_w, gap, avail, footer_h)
        if #pages > 1 then
            footer = self:pager(pages, content_w, gap)
        end
    end

    local padded = FrameContainer:new{
        bordersize = 0,
        padding = 0,
        padding_top = gap,
        padding_left = margin,
        padding_right = margin,
        margin = 0,
        VerticalGroup:new{ align = "left", top, body },
    }
    local footer_block
    local below_h = 0
    if footer then
        footer_block = FrameContainer:new{
            bordersize = 0,
            padding = 0,
            padding_left = margin,
            padding_right = margin,
            margin = 0,
            footer,
        }
        below_h = footer_block:getSize().h + 2 * gap
    end
    local spacer = math.max(0, screen_h - padded:getSize().h - below_h - nav_h)
    local page = VerticalGroup:new{
        align = "left",
        padded,
        VerticalSpan:new{ width = spacer },
    }
    if footer_block then
        table.insert(page, VerticalSpan:new{ width = gap })
        table.insert(page, footer_block)
        table.insert(page, VerticalSpan:new{ width = gap })
    end
    table.insert(page, navbar)
    for _i, row in ipairs(navbar.layout) do
        table.insert(self.layout, row)
    end

    self[1] = FrameContainer:new{
        width = screen_w,
        height = screen_h,
        bordersize = 0,
        padding = 0,
        margin = 0,
        background = Theme.color.surface,
        page,
    }
end

function Extensions:onClose()
    self:back()
    return true
end

function Extensions:onCloseWidget()
    if current == self then
        current = nil
    end
end

--- Closes Extensions and Home, then shows Home.
function Extensions:openHome()
    closeKaiaSurfaces(self)
    Home:show()
end

--- Closes Extensions and Home, which shows the file manager underneath.
function Extensions:openLibrary()
    closeKaiaSurfaces(self)
    local FileManager = require("apps/filemanager/filemanager")
    if not FileManager.instance then
        FileManager:showFiles()
    end
end

--- Closes Extensions and shows Kaia Notes.
function Extensions:openNotes()
    UIManager:close(self)
    require("kaia/notes"):show()
end

return Extensions
