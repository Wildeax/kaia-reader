# kaia-lua

Embedded LuaJIT and the KOReader compatibility layer (R7 of `docs/kaia/REWRITE_PLAN.md`,
spec in `docs/kaia/specs/LUA_COMPAT.md`).

- Stage L1: `LuaHost` runs KOReader's own pure Lua modules in KOReader's LuaJIT.
- Stage L2 and L3: KOReader widgets run in Kaia windows.
  `LuaUi` owns the host, the window stack and the scheduler.
- Stage L4: reader and library traits supply facts to Lua proxies. KOReader's
  PluginLoader loads plugins, and Kaia records their menus, actions and errors.
- Stage L5 part 1: desktop bundled-plugin workflows and explicit blockers are in
  `tests/bundled.rs` and `docs/kaia/inventory/PLUGIN_STATUS.md`. L5 is incomplete.

Run `. rust/kaia-lua/env.sh` (or `source tools/kaia/env.sh`) before cargo. The tests need the
KOReader emulator tree (`./kodev build`), its fonts and its `libs/`.
`kaia-book` and `kaia-doc` are test dependencies. The real-book tests use
`BookSession` for document state and `kaia-doc` for page text, which
`BookSession` does not expose as a public method. The library code depends on
neither crate, so the Rust app can implement the host traits without a cycle.

## Files

| Path | Role |
|---|---|
| `src/ui/mod.rs` | `LuaUi`: the public API, the stack, painting and input |
| `src/ui/bridge.rs` | The request queue and the `KAIA_UI` table that the shims call |
| `src/ui/foreign.rs` | `Foreign` (one shown widget) and its `ForeignRenderer` |
| `src/ui/input.rs` | Kaia gestures and keys as KOReader event objects |
| `lua/device.lua` | Shim for `device`: display, screen, input groups, capabilities |
| `lua/ui/uimanager.lua` | Shim for `ui/uimanager`: the KOReader API over `KAIA_UI` |
| `lua/kaia/events.lua` | Builds `Event`, `Geom`, `time` and `Key` objects from plain tables |
| `lua/kaia/paint.lua` | Paints a widget into a buffer that Rust owns |
| `lua/datastorage.lua` | Shim for `datastorage`: the data folder and its subfolders |
| `src/reader.rs` | `ReaderHost` and `LibraryHost` traits for document and library facts |
| `src/ui/plugins.rs` | Lua callbacks, plugin status, menu and action registries |
| `lua/apps/reader/readerui.lua` | ReaderUI, document and reader module proxies |
| `lua/apps/filemanager/filemanager.lua` | FileManager proxy over `LibraryHost` |
| `lua/dispatcher.lua` | Dispatcher registration into the Lua action table |
| `lua/kaia/plugin_runtime.lua` | KOReader PluginLoader setup, plugin instances and event calls |
| `tests/bundled.rs` | Bundled workflow tests and all 37 load outcomes |
| `tests/common/book.rs` | Real EPUB and PDF test hosts, annotation writes and layout changes |
| `lua/kaia/plugin_menu.lua` | Plugin menu callback and dialog adapter |
| `lua/kaia/plugin_compat.lua` | Adapter for the directory-defaults editor's reload call |
| `lua/kaia/reader_modules.lua` | Annotation, bookmark, TOC and book-info methods |
| `lua/kaia/reader_footer.lua` | Additional footer content in the foreign overlay |
| `tests/plugins.rs` | Bundled plugin loading, real EPUB and PDF menus, events, safe mode and failures |
| `tests/ui_widgets.rs` | InfoMessage, ConfirmBox, ButtonDialog, Menu, InputDialog, with goldens |
| `tests/ui_bridge.rs` | Event order, identity, scheduling, errors, input fields, refreshes, scale |
| `tests/golden/*.png` | Rendered frames. Regenerate with `KAIA_UPDATE_GOLDEN=1` |

## How a call travels

```text
widget code                  shim (lua/ui/uimanager.lua)      Rust (LuaUi)
UIManager:show(w, ...)  ->   KAIA_UI.show(w, x, y, flags,  )  queue Request::Show
                                                              (applied after the Lua call)
                                                              stack.show(kaia Window with
                                                              ForeignWidget + Renderer(Foreign))
                                                              -> Show event to w:handleEvent
frame()                      Renderer::paint(target)          Blitbuffer over target
                                                              -> w:paintTo(bb, x, y)
                                                              -> refreshes from setDirty/show/close
```

- **Queue.** Every shim function that changes state (`show`, `close`, `setDirty`,
  `scheduleIn`, `unschedule`, `sendEvent`, `broadcastEvent`, `forceRePaint`) records a
  `Request`. `LuaUi` applies the queue after each entry point returns: `exec`, `eval`,
  `gesture`, `key`, `send_named`, `run_due`, `frame`, and after each handler. No Lua
  call runs while the window stack or the scheduler is borrowed.
- **Identity.** A widget stays in the Lua registry (`RegistryKey`). Its table address
  identifies it, so `close(w)` removes the window that `show(w)` opened, and no other.
- **Windows.** `show` creates a `kaia_ui::Window` whose root is a `ForeignWidget`. The
  stack places it with the rules of `UIManager:show`: toasts above toasts, modal and
  normal windows as KOReader orders them, and `covers_fullscreen` as the paint plan uses it.
- **Painting.** `frame()` takes the paint plan from the stack and repaints the windows
  that are dirty. A window paints through a scratch `DirtyQueue`, so the refresh modes
  come from the Lua calls and keep their KOReader names. The refresh of an empty paint
  is the partial full-screen refresh that `_repaint` adds.
- **Refresh functions.** `setDirty(w, function() return mode, region end)` runs after
  the paint, as in KOReader.
- **Input.** Gestures and keys use `UIManager:sendEvent` order: toasts first (their
  answer is ignored), then the top window, then the always-active windows below it.
  A gesture becomes `Event:new("Gesture", ges)` with the `ges` fields of
  `frontend/device/gesturedetector.lua`: `ges`, `pos`, `start_pos`, `end_pos`,
  `relative` (pan gestures: current minus start), `direction` (compass name for pans
  and swipes, "horizontal", "vertical" or "diagonal" for pinch and spread, "cw" or "ccw"
  for rotate), `distance`, `span`, `start_span`, `angle`, `multiswipe_directions` (a
  space-separated string), and `time` (a `ui/time` value). `pos` and the other points
  are Geom objects built by `lua/kaia/events.lua`. A key becomes `KeyPress` or
  `KeyRelease` with a `device/key` object.
- **Scheduling.** `scheduleIn` puts a Rust task on the `kaia_events` scheduler. The task
  calls the Lua action once and drops its registry key. `nextTick` is `scheduleIn(0)`.
  `tickAfterNext` and the argument wrappers follow KOReader's code.
- **Screen.** `Device.screen` copies the methods of `base/ffi/framebuffer.lua`.
  `scaleBySize` and `scaleByDPI` use KOReader's formulas. A test compares them with
  KOReader's own framebuffer. `Screen.bb` wraps the frame memory on a grayscale panel,
  so `widgetRepaint` draws into the frame, as in KOReader.
- **Colour.** A colour frame is RGB24 in kaia-gfx, but KOReader's colour buffers are
  32-bit. KOReader's C blitter aborts when an alpha icon meets a 24-bit buffer, so a
  colour widget paints into an RGB32 scratch copy of the frame, and the copy is written
  back as RGB24. `Device.screen.bb` is RGB32 on a colour panel.

## Process-wide effects

- `LuaUi::new` sets the working directory to the KOReader tree. KOReader's icon code opens
  `resources/icons/...` relative to it, as `koreader.sh` expects. One process runs one
  `LuaUi`.
- The font list reads `./fonts` in KOReader. The device shim points it at the absolute
  `fonts` folder of the KOReader tree.
- The data folder gets the subfolders that KOReader's cache and history modules expect.

## Not reproduced

These differ from KOReader. Plugins that depend on them need an adaptation.

1. **Timing of effects.** `show`, `close`, `setDirty`, `scheduleIn`, `unschedule` and
   `sendEvent` take effect when the Lua call that made them returns, not inside the call.
   `isWidgetShown` and `getTopmostVisibleWidget` answer from the state at the start of
   the current call.
2. **Events during delivery.** `sendEvent` and `broadcastEvent` visit the windows that
   were on the stack when delivery started. KOReader rechecks the stack after each
   handler, so a window shown by a handler may get the event in KOReader and not here.
3. **Translucent widgets.** The `alpha` and `movable.alpha` checks in `setDirty` (which
   also dirty the windows below a translucent one) are not implemented.
4. **Cropping containers.** `widgetRepaint` and `widgetInvert` do not handle
   `cropping_widget`. They paint or invert the widget's own rectangle.
5. **Colour `widgetRepaint`.** On a colour panel `Screen.bb` is a separate buffer, so
   `widgetRepaint` does not reach the frame until the window is painted again.
6. **`forceRePaint`.** It marks every window for repaint. The paint happens at the next
   `frame()`, not now.
7. **Quit and power.** `UIManager:quit` is logged and ignored. The Kaia app owns the
   main loop. Power, suspend, Wi-Fi and frontlight are not in the device shim.
8. **Touch input switch.** `setIgnoreTouchInput` runs KOReader's `InputContainer`
   switch, so widgets stop taking gestures as in KOReader. Rust still delivers every
   gesture to Lua and does not block it.
9. **Modifiers and text.** Keys carry no modifier state. Text keys come as the
   upper-case key name, as KOReader's keyboard map gives them.
10. **Argument copies.** Named events sent from Rust copy their arguments (`Value`).
    Lua events sent from Lua pass the same tables to each handler, as KOReader does.
11. **Input hooks.** KOReader's HookContainer receives InputEvent for each Kaia key
    or gesture. Standby holds are counted, but no device power policy consumes them.
12. **Refresh dithering.** The `dither` flag reaches the queue. `DirtyQueue` drops it
    unless the panel supports hardware dithering.
13. **Colour panel format.** kaia-gfx has no 32-bit pixel format. The copy in
    `ui/foreign.rs` is the workaround. A native RGB32 format in kaia-gfx would remove it.
14. **Fallback widgets.** A widget that errors in `paintTo` or `handleEvent` leaves its
    last pixels on the frame. The error is recorded in `LuaUi::take_errors` and the UI
    keeps running. A failure in a plugin menu callback or reader event handler disables
    that plugin for the session and appears in `LuaUi::plugin_status()`.
15. **View modules.** `paint_reader_overlays` paints registered modules over a page
    through the foreign renderer. The app must call it before painting dialogs.
16. **Menu placement.** `MenuRegistry` stores each plugin's top-level entry and
    sorting hint. `open_plugin_menu` opens the plugin's KOReader submenu as a
    foreign window. The Rust app must place those entries in its menu.
17. **File manager fields.** The proxy supplies the folder, file list, open-book,
    copy, move, folder navigation and menu registration calls. The app supplies
    those operations through LibraryHost. Other methods raise a named Lua error.
18. **Reader modules.** Annotation writes use stable host ids. Statistics points to
    the loaded plugin instance. Book-info expansion handles title, author, series,
    page, chapter, path and date/time tokens. Unsupported tokens produce `N/A`.
    Folder shortcuts keep their registration in the Lua session.
19. **Actions.** Dispatcher executes through ActionRegistry. Plugin actions and
    font actions are registered. Unknown actions raise an error. The settings
    picker currently offers actions without arguments. Default reader actions,
    gesture override ordering and multiswipe recording need more work.
20. **Document defaults.** Plugins can replace `doc_settings.data`; flush writes
    its entries through ReaderHost. The app must deliver DocSettingsLoad before
    the first layout. Tests invoke that lifecycle step explicitly.

## Errors

A Lua error in a handler, a paint or a refresh function does not reach Rust. The call
returns "not used" (for events) or does nothing (for paints), and the message goes to
`LuaUi::take_errors()`. Errors in `LuaUi::exec` and `LuaUi::eval` are returned.
