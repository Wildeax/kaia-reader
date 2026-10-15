# Changelog

All notable changes to Kaia Reader are listed here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## 0.1.0 (unreleased)

First test release for the Kobo Libra Colour, on firmware 4.x. Kaia Reader is based on KOReader v2026.07.1. The Lua app is the default app. The Rust app is experimental. The known problems are in [docs/kaia/release/NOTES.md](docs/kaia/release/NOTES.md).

### Added

- Kobo packages for firmware 4.x and firmware 5.x. Each package holds the Lua app and the Rust app. Only the 4.x package has run on a device.
- Kaia Reader Rust, an experimental app. Its screens, gestures and reading defaults follow Nickel. It starts from its own NickelMenu entry, and it is not the default.
- Rust reader: text size that matches Nickel's large default, Atkinson Hyperlegible and OpenDyslexic fonts, font, size, weight and margin settings, hyphenation for 49 languages, highlight colors and styles, stylus ink, search inside a book with search history, double-tap lookup (off by default), a dictionary choice per book, PDF zoom with pinch, pan and fit modes, and CBR comics through libarchive.
- Rust library: scanning, covers, collections, favorites and tags, filters, search suggestions, an author view, storage by source, counts of finished books, and per-book statistics.
- Rust light and power: Quick settings, a sleep screen with book details, a standby timeout, a screen lock PIN, and airplane mode.
- Rust sync and export: reading progress sync, one-way WebDAV export of notes and highlights (off by default), OPDS catalogs, Calibre search and browsing, notes export as Markdown, JSON and PDF, screenshots, and two reset actions, Reset settings and Erase all Kaia data.
- Rust dictionaries: free dictionary downloads, off by default. Each downloaded file is checked against its SHA-256 hash.
- Rust import: KOReader's reading history, collections and statistics, Kobo's reading data from KoboReader.sqlite, and data from the Lua app.
- Rust network time, off by default.
- Lua app: Home, Library, Notes, Settings and Extensions screens, with one bottom bar. The Extensions screen lists each plugin with its compatibility status.
- Lua app: stickers in the margins of EPUB files and on PDF pages, with export and import.
- Lua app: opt-in Translate and Wikipedia lookups, off by default.
- Lua app: a one-time import of KOReader's settings, reading history, book data and plugin data.
- Lua app: extension API v1, with an example plugin.
- Safe mode in both apps. The Lua app shows a crash screen, and it stops restarting after five crashes in a row, each less than 60 seconds after the last.
- Kaia's own data folder, `.adds/kaia` for the Lua app and `.kaia` for the Rust app. Kaia does not change KOReader's own files.

### Changed

- Kaia hides KOReader's update menu and does not download KOReader packages. Kaia has no update channel, so each update is a package installed by hand.

### Fixed

- Rust frontlight: the slider showed 0 percent while the light was on. It now shows the level that the device reports.
- Rust Notes: with no notes, the navigation bar sat in the middle of the screen.
- Rust sleep: tapping Sleep in Quick settings could leave the device unresponsive. The fix follows KOReader's suspend sequence, stays awake when suspend fails, and logs each step. Its device test has not run.
- Rust library: a missing or unmounted books folder no longer removes its books from the index.
- Rust settings: a second instance of the app cannot overwrite the settings of the first. A damaged settings file is copied before any write.
- Rust touch: a hold gesture fired only when the finger lifted.
- Rust sync: an update queued during a drain could be dropped. Reads and writes now take a lock.
- Rust and Lua: a crash while scanning many books. The number of live CREngine documents is now capped.
- Rust CBR: reading late pages no longer rereads the whole archive.
- Lua app: with sidecars stored outside the book folder, KOReader's flush deleted the `.sdr` folder next to the book. Kaia never deletes or moves files in the book folder now.
