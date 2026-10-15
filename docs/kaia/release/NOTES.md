# Kaia Reader 0.1.0 release notes

Kaia Reader 0.1.0 is a test release for the Kobo Libra Colour. It is not a public release. The package has run on one Libra Colour, on firmware 4.42.23303, and most of its checks have not run on a device. Read the known problems before you install it, and back up the device first.

## Who this release is for

- Owners of a Kobo Libra Colour on firmware 4.x.
- People who can install NickelMenu and copy files to the Kobo over USB.
- People who will back up the device first and report what they see.

This release is not for:

- Other Kobo models.
- Firmware 5.x. The 5.x package is built, but no device run is recorded for it.
- The Kobo Store, Kobo account services and Kobo cloud sync. These stay in Nickel.
- Books with DRM. KOReader and Kaia do not open them.

## What Kaia is

Kaia Reader is an open source e-reader for the Kobo. It is a fork of KOReader v2026.07.1 and keeps KOReader's document engines, plugins and Kobo support. Kaia has its own folder, settings file and launch entry, so it installs next to KOReader and Nickel.

The package contains two apps:

- Kaia Reader is the default app. It runs KOReader's Lua frontend with Kaia's Home, Library, Notes and Settings screens and its stickers. Its library code is a Rust library that Lua calls through LuaJIT.
- Kaia Reader Rust is experimental. It is a new app written in Rust. Its screens, gestures and behavior follow Nickel, Kobo's own reading software, which is the design reference. It is not the default, and its device tests are not finished.

Kaia copies no Kobo code, icons, images, fonts or logos.

## What is new

Each item names its app when it is not in both. The Rust app items are in the experimental app.

### Reading

- Rust: The default text size and line layout follow Nickel's large default. On the reference page it sets nine body lines, as Nickel does. This was checked on the desktop, not yet on the panel.
- Rust: Reading fonts include Atkinson Hyperlegible and OpenDyslexic. Both fonts use the SIL Open Font License, and their license files ship in the package.
- Rust: Font, size, weight, margins and hyphenation are set in the font panel.
- Rust: Hyphenation covers 49 languages, using KOReader's TeX patterns. The language comes from the book, and you can override it.
- Rust: Highlights come in several colors and styles: underline, strikethrough, invert and lighten.
- Rust: Stylus ink on pages. Pen accuracy and latency are not measured yet.
- Rust: Search inside a book, with a saved search history.
- Rust: Double-tap to look up a word. This setting is off by default.
- Rust: Dictionary lookup, with a dictionary choice per book. See the known problems about Kobo's own dictionaries.
- Rust: PDF zoom with pinch, pan and fit modes.
- Rust: CBR comic archives, read through libarchive. The size of each unpacked page is capped.
- Rust: Tap zones. The left third goes back a page, the centre opens the reading menu, and the rest goes forward. A tap in the top right corner adds a bookmark.
- Lua: All of KOReader's reading features are there, as in KOReader.

### Library

- Rust: Scans the books folder and keeps an index in SQLite. Hidden program folders are skipped.
- Rust: Covers, collections, and favorites and tags.
- Rust: Filters, sorts, layout memory and search suggestions. The default sort is Recent.
- Rust: Author view, storage by source, and counts of finished books.
- Rust: Per-book statistics in the book details, showing the days read.
- Rust: A missing or unmounted books folder does not remove books from the index.
- Rust: Calibre search and browsing of the metadata file that the device downloads. See the sync and export section.

### Notes

- Lua: Stickers. Place an image sticker or a text note in the margin of an EPUB, and place stickers on PDF pages. Export and import stickers, and repair orphaned stickers from Notes.
- Rust: Stickers, and the same annotation store as the Lua app.
- Rust: Notes export as Markdown, JSON and PDF.

### Light and power

- Rust: Quick settings opens from the Light button in the reading menu. It holds the frontlight, warmth, Wi-Fi, airplane mode, orientation, and Sleep.
- Rust: The frontlight and warmth sliders show the right levels on the Libra Colour. The first device run did not record which warmth node the warmth setting writes to.
- Rust: The sleep screen shows the cover and details of the current book. It has not been checked on either panel mode.
- Rust: A standby timeout, set in minutes. Its power draw has not been measured.
- Rust: Automatic sleep only dims the screen for now. See the known problems about sleep.
- Lua: Sleep and wake work on the device. The Lua app writes Nickel's light settings by default, as KOReader does.

### Sync and export

- Rust: Reading progress sync, using KOReader's sync protocol. It has not been tested against a real sync server.
- Rust: One-way WebDAV export of notes and highlights. It is off by default. Each export writes one Markdown file and one JSON file per book, and nothing syncs back. It has been tested only against a local WebDAV server.
- Rust: OPDS catalogs. Browse, search and download books. Not yet tested against real catalogs on the device.
- Rust: Calibre search and browsing of the downloaded metadata file. The wireless Calibre connection has not been checked on the device.
- Rust: Imports KOReader's reading history, collections and statistics, and Kobo's reading data from KoboReader.sqlite. The Kobo import has not been checked against a real Kobo database.
- Rust: Imports data from the Lua app. Open Settings, then Data.
- Lua: One-time import of KOReader's settings, reading history, book data and plugin data.

### Privacy

- Kaia does not use the network unless you turn on a feature that needs it.
- Features that use the network are sync, WebDAV export, OPDS, Calibre, network time, Translate and Wikipedia lookups, and dictionary downloads. Each one stays off until you turn it on.
- Translate and Wikipedia lookups are off by default. Turning them on shows a warning that the selected text leaves the device.
- Dictionary downloads are off by default. Each downloaded file is checked against its SHA-256 hash.
- Network time is off by default.
- Kaia has no telemetry and no analytics. It does not update itself.
- Screen lock with a PIN.
- Reset settings and Erase all Kaia data. Both ask you to type to confirm. Neither touches book files, Nickel or KOReader.

## Experimental

- Kaia Reader Rust is experimental. It is not the default. The Rust app becomes the default only after its device tests pass.
- Automatic sleep only dims the screen. Kernel suspend for automatic sleep is off until the device sleep test passes. Manual Sleep and the Power button still request a real suspend.
- Stylus ink, PDF zoom, sync, WebDAV export, OPDS, Calibre and the Kobo import have code and desktop tests. They have not been checked against real servers or on the panel.

## Known problems

### Sleep and power

- On 2026-10-15 the Rust app stopped responding after Sleep in Quick settings. A short Power press did nothing, and holding Power restarted the device into Nickel. No data was lost. The fix is in commit a87f4df3c. Its device test has not run.
- Automatic sleep only dims the light. This uses more battery than suspend. Return to Nickel before you leave the device unattended.
- Battery use and suspend behavior are not measured. The 500 suspend cycles and the week-long soak test have not run.

### Reading

- Page-turn time on the panel is not measured. A desktop run of the Rust app failed the 100 ms target for EPUB, with median page turns of 250 to 320 ms. That run came before a later change to the ink code.
- Kobo's own dictionaries in `.kobo/dict` are encrypted, so Kaia cannot use them. A lookup says "No dictionary installed" and does not give the reason. The free dictionary downloads are the other option, and they are off by default.
- While a PDF is zoomed, a tap does not turn the page at the same zoom as Nickel, and Kaia has no page map yet. Pinch and pan were checked on the panel.
- In the Rust app, the reading menu, the selection sheet, the settings pages and the light panel differ from Nickel in layout and size. Settings sub-pages have no tab bar. They use a back arrow at the top left and Previous and Next buttons, where Nickel scrolls one list.
- The Rust app starts in portrait. Sensor-driven rotation is not connected, so use the orientation setting in Quick settings.
- Lua app stickers on PDFs can cover text, because they go where you select text. Fixed-layout pages have no margin placement. In a narrow margin, an image sticker is drawn faded over the end of its line.

### Import and data

- The Kobo import has not run against a real KoboReader.sqlite. Reading positions from Nickel are not imported.
- Reset settings and Erase all Kaia data have warning text and recovery steps that are still open.

### Install and launch

- Nickel lists Kaia's own files as books unless you add the ExcludeSyncFolders line. The install guide covers this step.
- Firmware 5.x has no device run. NickelMenu does not support 5.x, and the KFMon route is not verified.
- The package has no Kaia icon or artwork.
- Kaia has no update channel. Each update is a package you install by hand.
- The recovery procedure for a failed app start is written. It has not run on the device.

### Language

- New Kaia strings are in English only. KOReader's own strings appear in the language you chose.

### Plugins

- Some KOReader plugins do not work in the Rust app. 9 of the 37 bundled plugin entries and 6 of the 9 community plugins are blocked there.

### Not in this release

- Japanese, Chinese and Korean input. Latin keyboards only.
- Read aloud and screen reader support. The firmware has no offline speech engine.
- Handwriting recognition. Ink works, and recognition is paused.
- Bluetooth remotes and audio.
- Ambient light sensing.
- Sensor-driven rotation.
- The Kobo Store, Kobo account sync, cloud links, OverDrive and store books.

### Not checked on the device

- Sync against a real server, WebDAV export to a real server, OPDS downloads and the wireless Calibre connection.
- Panel refresh and ghosting, touch in every corner and orientation, and the frontlight and warmth nodes.
- Power draw, suspend over a night, and recovery from power loss or a full volume.
- Firmware 5.x.

## Install

Read [INSTALL_KOBO.md](../INSTALL_KOBO.md) before you start. It has every step. In short:

1. Back up the Kobo. The backup guide is [BACKUP_AND_RECOVERY.md](../device/BACKUP_AND_RECOVERY.md).
2. Check your firmware version. The package for 4.x is named `kaia-kobo-<version>-<hash>.zip`. The package for 5.x is named `kaia-kobo-v5-<version>-<hash>.zip`.
3. Install NickelMenu, following its own documentation.
4. Connect the Kobo over USB and unzip the package at the root of the Kobo storage. Do not delete `.adds/koreader`.
5. Add the ExcludeSyncFolders line to Nickel's configuration, as the install guide describes.
6. Eject the Kobo, unplug the cable, and start "Kaia Reader" from NickelMenu.

## Go back to Kobo's software

- To leave the Lua app, use its exit from the menu. Nickel returns.
- To leave the Rust app, press both page buttons together. Nickel returns. Connecting a computer also leaves the Rust app.
- To remove Kaia, follow the uninstall steps in [INSTALL_KOBO.md](../INSTALL_KOBO.md). Delete `.adds/kaia` and `.adds/nm/kaia`. Books stay on the device.
- The Rust app's data folder, `.kaia`, stays after uninstall. Back it up, or delete it by hand.
- Uninstalling does not remove the ExcludeSyncFolders line from Nickel's configuration. Remove it by hand if you want Nickel to show hidden folders again.
- The Lua app writes Nickel's light settings by default. Brightness set in Kaia can show in Nickel.

## Report a problem

File a bug at [github.com/Wildeax/kaia-reader/issues](https://github.com/Wildeax/kaia-reader/issues). Include:

- Kaia version 0.1.0, and the firmware version.
- The steps you took, what you expected, and what happened. Photos help.
- The log for the app you used. The Lua app writes `.adds/kaia/crash.log`. The Rust app writes `.kaia/rust.log`.

Remove personal information and account identifiers from logs before you post them. Do not post serial numbers or network credentials. Report security problems privately, as [SECURITY.md](../SECURITY.md) describes.

The support boundaries are in [SUPPORT.md](../SUPPORT.md).
