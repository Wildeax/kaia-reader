# First device run, Kobo Libra Colour

Date: 2026-10-14. Device: Kobo Libra Colour, firmware 4.42.23303, new out of the box. Package: `kaia-kobo` 4.x, built by CI from 612c493b6 and then 796117d51. Launcher: NickelMenu v0.6.0.

The owner ran the Lua app and the Rust app by hand. The orchestrator then drove the device over Wi-Fi: SSH with a key only, screen capture from `/dev/fb0`, and synthetic touches and button presses written to the input devices. Nickel screen captures are Kobo's screens and stay outside the repository; `nickel/SCREENS.md` describes them in Kaia's words.

## What worked

- Install: the package unzips into `.adds/kaia` and `.adds/nm/kaia` only. NickelMenu shows both entries. The NickelMenu entry syntax matches the NickelMenu documentation.
- Lua app: starts, opens EPUB and PDF, turns pages by touch and buttons, sleeps and wakes, and exits to Nickel from the menu.
- Rust app: starts in about 20 seconds on the first run, scans the three test books, opens EPUB, turns pages, shows every main screen.
- Rust app exit: connecting USB saves and returns to Nickel. Both page buttons together exit to Nickel. Nickel restarts with the right depth and rotation. The log shows no errors.
- Rust library scan ignores the hidden program folders.
- Framebuffer: 1264 x 1680, 32 bpp RGBA, stride 5056, rotation 1 (canonical upright), MediaTek quirks on. Touch: Elan, `ABS_MT_POSITION_X` 0 to 1680 and `ABS_MT_POSITION_Y` 0 to 1264; KOReader's swap and y mirror map it to the screen. Page buttons: keys 193 (back) and 194 (forward) on `gpio-keys`.

## Problems found

| Problem | Effect | Status |
|---|---|---|
| `tools/kaia/device/backup.sh` stopped on macOS, which refuses to list `.Spotlight-V100` on the volume. | No backup. | Fixed in 8b21616e3. |
| Nickel imported about 120 files from `.adds/kaia` (icons, help pages) as books. | Nickel's library shows 129 books instead of 3. | Open. The usual fix is `ExcludeSyncFolders` in Nickel's config, as KOReader's install guide says. `INSTALL_KOBO.md` must say so. |
| Rust app: the frontlight slider shows 0% while the light is at 26%. `mxc_msp430.0/brightness` reads 0 on this device; `actual_brightness` holds the level. | Wrong slider; the light may jump when moved. | Being fixed. |
| Rust app: on Notes with no notes, the navigation bar sits mid-screen under the empty text. | The bottom of the screen does nothing. | Being fixed. |
| Rust app: Kobo's own dictionaries in `.kobo/dict` are encrypted (the word index is plain, every entry is not). | Kaia cannot use them, by design. The lookup says "No dictionary installed" instead of naming the cause. | Decision B-82 holds: offer the free downloads. The message needs a fix. |
| Rust app: default text is about half Nickel's size (17 lines a page against 9; the book is 248 pages against 96). Kaia also hyphenates and does not justify by default, and shows only "Page x of y" at the bottom. | The owner prefers Nickel's large default. | Nickel defaults study running; see `nickel/DEFAULTS.md` when done. |
| Rust app: the reading menu, selection sheet, settings pages and light panel differ from Nickel in layout and size. | Feels unlike Nickel. | See `nickel/GAPS.md`. |
| Rust app settings sub-pages have no tab bar and use a back arrow at the top left. Lists are paged with Previous and Next buttons; Nickel scrolls one list. | Different navigation. | In `nickel/GAPS.md`. |

## Nickel behavior seen on the device

- Sleep after 5 minutes, power off never. The sleep screen shows the current book's cover with a details panel.
- Nickel turns Wi-Fi off when it sleeps.
- PDF: pinch out zooms around the fingers with no chrome. A tap while zoomed turns to the next page at the same zoom and offset. The menu while zoomed shows a page map with the visible area, the page number and the zoom percent, and a move tool with a zoom slider.
- List rows react to a press of about 180 ms; icons react to a shorter tap.

## Next device checks

- Sleep screen with book details (B-140) and standby (B-141) on both panel modes.
- PDF zoom (B-137) against Nickel's behavior above.
- Frontlight and warmth after the fix.
- Power draw at rest and while reading, suspend and resume over a night.
