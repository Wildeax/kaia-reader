# Kaia extension API

Version 1 lives in `frontend/kaia/api/v1.lua`. The example plugin is
`plugins/kaiaexample.koplugin`.

The API lets a plugin describe the commands, menu items, settings and dialogs
it adds. Kaia builds the KOReader tables for them, so the menus and dialogs
look the same across plugins. Existing plugins keep their own code. Using the
API is optional.

## Security boundary

This API is a convenience layer, not a sandbox.

A plugin that uses the API runs as trusted code. It has the same access as any
other plugin: the full Lua environment, FFI, the file system and the network.
Checks in this API only catch wrong arguments. They do not limit what a plugin
can do. A permission list in a plugin manifest would not change this either.
See `docs/kaia/ENGINEERING_PLAN.md`, section 5.

Do not install plugins from untrusted sources on the strength of this API.

## Getting the API

```lua
local KaiaAPI = require("kaia/api/v1")
local api, reason = KaiaAPI.require(1)
if not api then
    error(reason)
end
```

`KaiaAPI.require(major)` returns the API table for a major version this build
provides. For any other version it returns `nil` and a reason. Check
`api.version` if you need the exact version number.

Every registration function takes `plugin = self`, where `self` is the plugin
instance. The API uses `plugin.name` in its keys and `plugin.ui` as the argument
to your `run` functions. Give plugin ids as identifiers: they start with a
letter and use letters, digits and underscores.

Invalid arguments raise an error that names the field, for example
`KaiaAPI.addCommand: 'title' must be a non-empty string`.

## Functions

### addCommand

```lua
local handle = api.addCommand{
    plugin = self,
    id = "say_hello",
    title = _("Say hello"),
    run = function(ui) ... end,
    event_name = nil,   -- optional
}
```

Registers a Dispatcher action, so a user can bind the command to a gesture or a
quick menu. `run` is called with `plugin.ui`. Errors in `run` go to the log.

The action name is `kaia_ext_<plugin>_<id>`. The event defaults to
`KaiaApi_<plugin>_<id>`, and the plugin instance gets an `on<event>` method.

Returns a handle with `id`, `action`, `event` and `title`.

### addMenuItem

```lua
api.addMenuItem{
    plugin = self,
    id = "greet",
    title = _("Greeting"),
    section = "tools",         -- "tools", "settings" or "more_tools" (default)
    run = function(ui) ... end,
    -- or, instead of run:
    -- sub_items = { { title = _("First"), run = function(ui) ... end }, ... },
    enabled_func = function() return true end,   -- optional
}
```

Adds an item to the main menu through `plugin.ui.menu:registerToMainMenu`.
Give either `run` or `sub_items`, not both. The item goes to the end of the
section, as KOReader orders the rest of the menu.

Sections:

- `tools`: the Tools tab.
- `more_tools`: the More tools submenu of the Tools tab.
- `settings`: the Settings tab.

Returns a handle with `id` and `key`.

### addSetting

```lua
local get, set = api.addSetting{
    plugin = self,
    id = "shout",
    title = _("Shout the greeting"),
    kind = "toggle",           -- "toggle", "choice" or "number"
    default = false,
    -- for kind = "choice":
    -- choices = { "short", { value = "long", text = _("Long") } },
    -- for kind = "number", both bounds are required:
    -- min = 1, max = 10,
    section = "settings",      -- optional, default "settings"
}
```

Stores the value in `G_reader_settings` under `kaia_ext_<plugin>_<id>`, and
adds a menu item that edits it. The getter returns the stored value when it has
the right type and range. Otherwise it returns `default`. The setter checks the
value and raises an error if it does not fit the setting.

- `toggle`: `default` is a boolean.
- `choice`: `choices` is a non-empty array. Each entry is a string, or a table
  with `value` and `text`. `default` must be one of the values.
- `number`: `default`, `min` and `max` are numbers, with `min <= default <= max`.
  The menu opens a spin widget with a restore-default button.

Returns the getter and the setter.

### showMessage

```lua
api.showMessage{ text = _("Saved."), kind = "info" }   -- or "warning"
```

Shows an information message box. Returns the widget.

### confirm

```lua
api.confirm{
    text = _("Delete the cache?"),
    ok_text = _("Delete"),     -- optional, default "OK"
    cancel_text = _("Keep"),   -- optional
    on_ok = function() ... end,
}
```

Shows a confirmation box. `on_ok` runs only when the user taps the OK button.

## Strings and translation

The API shows the text you pass in. Pass strings through `gettext` in your
plugin, as `_("...")`. The API does not translate them.

## Turning a plugin on or off

Kaia's Extensions screen turns plugins on and off by setting `plugins_disabled`,
the same setting the plugin manager uses. A change takes effect after a
restart.

## Example

`plugins/kaiaexample.koplugin` uses every function once. It is disabled by
default. Set `DISABLED_BY_DEFAULT` to `false` in its `main.lua` to load it.
