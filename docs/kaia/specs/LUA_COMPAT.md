# Lua plugin compatibility layer (R7)

Status: app integration implemented, version 1. L5 workflows remain partial. Goal: KOReader plugins run in the Rust app. First target: the applicable bundled plugins and a named, tested list of community plugins (`docs/kaia/inventory/plugins.md`).

## Approach

Run KOReader's own Lua code wherever it does not own the screen, input or the event loop, and replace only the modules that do. Plugins then call the same modules they call today.

```text
Rust app (kaia-app)
  kaia-lua
    LuaJIT (KOReader's pinned build, one runtime, main thread only)
    package.path: kaia shims first, then KOReader's frontend/ and plugins/
    shims (Lua + Rust):
      device, device.screen      -> Kaia display info; Screen backed by a Blitbuffer per foreign widget
      ui/uimanager               -> kaia-events WindowStack, Scheduler, DirtyQueue
      ui/event                   -> kaia_events::Event
      apps/reader/readerui       -> proxy over kaia-book's BookSession and the Reader surface
      apps/filemanager/filemanager -> proxy over the Library surface
      document objects           -> proxy over kaia-doc / kaia-book (KOReader Document API)
      dispatcher                 -> kaia-events ActionRegistry
      main menu registration     -> kaia-events MenuRegistry
      luasettings, docsettings   -> KOReader's own modules, with paths into Kaia's data folder
    unchanged KOReader modules: util, ffi/util, dump, datetime, gettext, logger, json, sort,
      widgets (ui/widget/*), fonts, blitbuffer (base/ffi), text rendering (libkoreader-xtext)
```

## Widgets

KOReader widgets draw into a Blitbuffer and handle KOReader gesture tables. Kaia does not translate them. A plugin dialog is a real KOReader widget tree running inside a `kaia_ui::ForeignWidget`:

- `UIManager:show(widget)` (shim) creates a Kaia window whose root is a `ForeignWidget` sized to the widget's `dimen` (or full screen), with the modal flag KOReader uses.
- The foreign renderer gives the widget a Blitbuffer that wraps the window's pixmap region, calls `widget:paintTo(bb, x, y)`, and turns `UIManager:setDirty` calls into Kaia dirty regions with the same refresh modes.
- Kaia gestures are converted to KOReader `ges` tables (`ges`, `pos` as a Geom, `direction`, `distance`, `time`) and delivered through `widget:handleEvent(Event:new("Gesture", ges))`; keys become KOReader key events.
- Fonts and sizes come from KOReader's own `ui/font` and `ui/size` modules, so legacy dialogs look like KOReader until a plugin is adapted. `docs/kaia/specs/DESIGN.md` tokens are applied through `ui/size` values as the Lua app already does.

## Reader and file manager proxies

Plugins reach `self.ui` (a ReaderUI or FileManager). The proxies implement the fields and methods that plugins use, found by the static inventory (`tools/kaia/inventory/plugin_inventory.py`) and the runtime traces (`frontend/kaia/trace.lua`):

- `self.ui.document`: page count, current page, `getPageText`, `getTextFromPositions`, `getScreenBoxesFromPositions`, xpointer methods, `getProps`, `file`, `is_pdf`, `info`.
- `self.ui.doc_settings`: a DocSettings object over Kaia's per-book state, with KOReader's keys (`annotations`, `percent_finished`, `summary`, `doc_props`).
- Reader modules plugins call: `self.ui.rolling` and `self.ui.paging` (go to page, current page), `self.ui.view` (`registerViewModule` for overlays drawn through a foreign layer above the page), `self.ui.highlight`, `self.ui.annotation`, `self.ui.bookmark`, `self.ui.toc`, `self.ui.menu` (`registerToMainMenu`), `self.ui.statistics`.
- Plugins become reader modules: `PluginLoader` (KOReader's own, with Kaia's path list) instantiates them and they receive events (`onReaderReady`, `onPageUpdate`, `onCloseDocument`, `onSuspend`, `onResume`) from the Rust app through the shim UIManager's broadcast.

## Rules

- One LuaJIT runtime on the main thread. Rust calls Lua only from the main loop. Lua calls Rust through the shim functions, which never re-enter the main loop.
- Lua errors are caught at every boundary. A plugin that raises is disabled for the session, logged, and shown in Extensions as failed.
- Plugins are trusted code with full access (plan section 5). No sandbox claim.
- Safe mode skips all third-party plugins.

## Stages

| Stage | Deliverable | Check |
|---|---|---|
| L1 | `kaia-lua`: embed LuaJIT, paths, Value conversion, error handling; run pure KOReader modules | util, dump, luasettings, docsettings, datetime tests pass under Kaia |
| L2 | Shims: device and screen, uimanager, event, gettext, logger | A KOReader `InfoMessage` and `ConfirmBox` show in a Kaia window and respond to taps |
| L3 | Foreign widgets and input mapping | KOReader `Menu`, `ButtonDialog`, `InputDialog` with the Kaia keyboard work |
| L4 | ReaderUI and FileManager proxies, document API, plugin loader, menus, dispatcher | `hello.koplugin` (forced on), `exporter`, `statistics` open their menus and dialogs |
| L5 | Bundled plugin workflows from the inventory, then the community list | Each workflow in `plugins.md` passes on the desktop |

L4 status: implemented in `rust/kaia-lua`. The `tests/plugins.rs` checks load
KOReader's PluginLoader, open exporter and statistics menus against a real EPUB
and PDF, deliver reader events, and check safe mode and plugin failure status.
The Rust app now connects its reader session and library index to these traits.


L5 part 1 status: partial. The bundled tests exercise file exports, annotation
writes, statistics views, vocabulary review, settings dialogs, gesture dispatch,
profiles, timers, archive operations and text editing. All 37 inventory entries
have explicit load-outcome assertions. ReaderHost now accepts annotation writes
and settings flush; LibraryHost accepts copy, move and folder navigation.
Registered view modules and additional footer text can paint through
`LuaUi::paint_reader_overlays`.

The app connects these methods, overlays and lifecycle events. Device callbacks
use kaia-platform for battery, charging, frontlight, warmth and suspend requests.
The app idle policy honors pause_auto_suspend and standby holds. Dictionary
lookups and plugin buttons use the native dictionary panel. Selection context,
coverbrowser's native library adapter and terminal process ownership remain
incomplete. Server and hardware operations are
excluded from desktop tests. See [the per-plugin status table](../inventory/PLUGIN_STATUS.md)
for test names, untested baseline steps and changes needed outside kaia-lua.

## App runtime

The default `lua` Cargo feature enables plugin support. A no-Lua build uses
`--no-default-features --features desktop` or `--features kobo`. The Kobo build
script adds `lua` to its normal `kobo` app build and links the pinned target
LuaJIT from the KOReader staging tree. Packages include its shared library and
SONAME, the frontend, bundled plugins and the shims under `lua/`.

Settings, Extensions enables the runtime lazily. One LuaUi stays on the app
thread for the session. Bundled plugins load from the install tree; user plugins
load from `<data>/plugins`. The per-plugin switches persist in settings.json.
Bundled entries marked works or works with limits default to on. Other entries
default to off. The global enable switch defaults to off.

`--safe-mode` and the safe-mode setting both skip user plugins. A caught plugin
error removes its menus and callbacks for the session and appears in Extensions.
Callbacks from an older plugin load do not run after reload. Foreign dialogs
share the native window stack and receive its ordered input. View modules and
footer content paint between the reader page and dialogs.

Reader events include ReaderReady, PageUpdate, CloseDocument, Suspend, Resume,
FlushSettings, DocumentRerendered and annotation count changes. Charging changes
reach plugins in reader and library contexts. Calls that need the session wait
until the reader worker returns it. DocSettingsLoad currently runs after the
session opens, so applying directory defaults before first layout remains open.
Arbitrary per-book plugin values live in `<data>/plugin-books/<partial-md5>.json`.

The app tests in `rust/kaia-app/tests/plugins.rs` exercise hello, exporter,
statistics, perceptionexpander, systemstat, batterystat, vocabulary builder,
readingstreak and Anki, plus failure and device fixtures. Those tests make no
server requests. They do not establish physical Kobo behavior or full community
plugin compatibility.
