# KR-010: Hybrid UI proof

Kaia Home and an unchanged plugin dialog share one window stack and one UIManager run loop. This page records what the desktop emulator test and screenshots show. Device checks are still open (see the last section).

Scope: the layering, input, rotation and suspend behavior of Home under a dialog. The plugin dialog is a `ConfirmBox` built the way plugins build one. No plugin code changed.

## What was tested

Spec: `spec/unit/kaia_hybrid_ui_spec.lua`, five cases.

1. Showing Home registers no timer or task. The spec wraps `schedule`, `scheduleIn`, `nextTick` and `tickAfterNext` during `Home:show()`, and checks the count is 0 and the task queue length is unchanged.
2. A dialog on top of Home takes input. The spec paints the dialog, finds the Cancel button, and sends a tap at its center with `UIManager:sendEvent`. The cancel callback runs once and the dialog closes.
3. After the dialog closes, Home is the topmost visible widget again.
4. Rotation. The spec calls `Screen:setRotationMode`, broadcasts `SetDimensions`, calls `UIManager:onRotation`, and repaints Home and the dialog. Nothing errors and the dialog stays on top.
5. A `Suspend` and `Resume` broadcast pair leaves Home and the dialog shown, the dialog on top, and both repaintable.

The spec restores the rotation mode and closes both widgets after each case.

Screenshot action scripts live in `tools/kaia/shot/dialogs/`. They run through `tools/kaia/screenshot.sh` at 1264x1680.

## Results

| Command | Result |
| --- | --- |
| `./kodev test -b front kaia_hybrid_ui` | 5 of 5 cases OK |
| `./kodev test -b front` | 88 of 88 OK, `Fail: 0` |
| `luacheck` on the spec, `frontend/ui/size.lua`, `frontend/apps/filemanager/filemanagermenu.lua`, `tools/kaia/shot/dialogs/*.lua` | 0 warnings, 0 errors |

The full front run includes the upstream specs that check sizes. None failed after the `size.lua` change.

The 0.2 s spec time is test time in the emulator. It does not measure paint cost. KR-010 asks for performance numbers, and those need the device.

## Screenshots

All files are in `/tmp/kaia-dialogs/`. Each pair shows the same dialog before and after the `size.lua` change. The `-- Kaia:` comments in `frontend/ui/size.lua` record the upstream values.

- `hybrid.png`: a ConfirmBox over Kaia Home with an empty history. Home's header, "No books yet" block, Open library button and nav bar show around the dialog. The dialog is centered with Cancel and Remove visible.
- `confirm-box-before.png`, `confirm-box-after.png`: a two-line ConfirmBox over the Library. Text and both buttons fit.
- `button-dialog-before.png`, `button-dialog-after.png`: a ButtonDialog with a title and four buttons in a 2x2 grid over the Library.
- `info-message-before.png`, `info-message-after.png`: a two-line InfoMessage over the Library.
- `file-manager-menu-before.png`, `file-manager-menu-after.png`: the file manager main menu (TouchMenu) in the Settings tab, showing "Start with: Kaia Home". The new "Kaia navigation bar in Library" item is below the visible area and was checked separately.
- `multi-input-before.png`, `multi-input-after.png`: a MultiInputDialog with three fields and Cancel and Save.
- `reader-config-before.png`, `reader-config-after.png`: the reader's bottom config panel on `1342.epub`, opened by `ReaderConfig:onShowConfigMenu()`.

Action scripts: `tools/kaia/shot/dialogs/hybrid.lua`, `confirm-box.lua`, `button-dialog.lua`, `info-message.lua`, `file-manager-menu.lua`, `multi-input.lua`, `reader-config.lua`.

## Findings

- Home does not relayout on rotation. Home has no `SetDimensions` or `onSetRotationMode` handler, so it keeps the layout built at construction. The spec checks that repainting after a rotation works and that the stack is unchanged. It does not check a relayout, because there is none. The fix belongs in `frontend/kaia/home.lua`: rebuild the content on `SetDimensions` at the new size.
- Two upstream dialogs have no visible close control. ButtonDialog closes on a tap outside it. InfoMessage closes on a tap or after its timeout. Both are upstream behavior and were not changed.
- Home starts no timers. The spec proves this for `Home:show()`. Home's refresh path was not covered by the spec.

## Needs the device

- E Ink refresh: check that the dialog appears and disappears on Home with no flash beyond the normal refresh, and that the Home area redraws cleanly after the dialog closes.
- Ghosting: open and close the dialog over Home and over the Library many times, then check for leftover outlines of the dialog.
- Page-turn keys: Home handles only the Back key (`Close`) when the device has keys. Check that page-turn keys do nothing to Home while a dialog is open.
- Color and grayscale: check the 12-unit window and 10-unit button radii, and the thinner border, on the Libra Colour panel with color rendering on and off.
- Suspend and resume: suspend the device with the dialog open, resume, and check that the screen redraws the dialog and Home without a blank frame.
- Hardware rotation: check Home after rotation on the device, once Home handles `SetDimensions`.
- Touch: check tap-outside dismissal for ButtonDialog and InfoMessage on the touch panel.
- Performance: measure the time to show Home and the time to show a dialog over Home, on the device, with the reference settings from ENGINEERING_PLAN section 8.
