# Owner decisions

These are the items in group C of `BACKLOG.md`. The project owner decided B-81 to B-99 on 2026-10-14. Each row records the choice and what it means for the build. Items marked "build" are in group A of `BACKLOG.md` as B-120 and later.

Rules that apply to every row: no network access unless the user turns the feature on, no telemetry, and no code or data whose license Kaia cannot ship.

## Decided

| Id | Decision | Choice | What follows |
|---|---|---|---|
| B-81 | WebDAV export of notes and highlights (T2-4) | One-way export, off by default | Build. One Markdown file and one JSON file per book, written on Export or when a book closes. Nothing syncs back. |
| B-82 | Dictionaries (T2-6) | Kobo's local dictionaries and free downloads | Build. Kaia reads the dictionaries Nickel already put on the device, if they are readable (a device check). It also offers the free Wiktionary-based dictionaries from KOReader's list as an opt-in download, off by default, each file verified by SHA-256. Nickel's dictionaries are never downloaded or redistributed by Kaia. |
| B-83 | CBR comics (T3-15) | libarchive | Build. libarchive is BSD licensed and already in KOReader's build. The unpacked size per page is capped. |
| B-84 | Factory reset (T3-18) | Two scoped buttons | Build. "Reset settings" and "Erase all Kaia data". Neither touches book files, Nickel or KOReader. Both need a typed confirmation and suggest a backup. |
| B-85 | Japanese, Chinese and Korean input (T3-4) | Leave out of the first release | Closed for now. Latin keyboards only. |
| B-86 | Hyphenation (T3-7) | KOReader's TeX patterns | Build. The hyph-utf8 patterns KOReader ships, the same source Nickel uses, chosen by the book's language with a manual override. |
| B-87 | Notebook export (T3-16) | Add PDF export | Build. A PDF of notes and highlights per book, written with MuPDF, which Kaia already links. Word and image archives stay out. |
| B-88 | Read aloud and screen reader (T3-2) | Leave out for now | Closed for now. The firmware has no offline speech engine and the device has no speaker. |
| B-89 | Handwriting recognition (T3-1) | Leave out, keep ink | Closed. Nickel's engine (MyScript) is closed and its license does not fit AGPL. |
| B-90 | Network time (T3-13) | NTP, off by default | Build. A switch that sets the clock from pool.ntp.org when Wi-Fi is on. |
| B-91 | Lua app writing Nickel's brightness (issue 6.3) | Keep writing by default | No code change. Record in `specs/STORAGE.md` that the Lua app writes Nickel's light settings by default, as KOReader does. |
| B-92 | Translate and Wikipedia (issue 6.1) | Keep both, opt-in | Build. Each one off by default behind a switch that warns the selected text leaves the device. |
| B-93 | Update channel | Signed GitHub releases now, in-app updater later | Build. Signed packages with checksums on GitHub Releases for the first releases. An opt-in in-app updater with signature checks and rollback comes after device testing. |
| B-94 | Product name | Keep "Kaia Reader", no search | Closed. |
| B-95 | Removed books (T1-18) | Keep reading data forever | Build. Reading data of a removed book is never deleted automatically. |
| B-96 | Ambient light sensor (T3-19) | Experiment after device tests | Moved to group B. Check the sensor on the device first; if it works, add opt-in auto brightness. |
| B-97 | Experimental features (T3-5) | Put new features there first | Build. Large new features ship behind an experimental switch until their device tests pass. |
| B-98 | Reading and library gaps | Build all four | Build. In-book search history; per-book statistics; Home controls and double-tap lookup; favorites and tags; extra PDF zoom modes. |
| B-99 | Device gaps | PIN, screenshots, sleep screen and standby | Build. Screen lock PIN; screenshots; sleep screen with book info; standby timeout. Captive portal login stays out. |

## Later, after device testing

| Id | Decision | When |
|---|---|---|
| B-100 | Final memory and speed budgets | After the measurements in group B |
| B-101 | Architecture go or no-go for each remaining ADR | After device, data and plugin results |
| B-102 | Release and maintenance owners, signing approver | Before the first public release |
