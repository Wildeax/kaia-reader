# Kaia app: surfaces and flows (R1, R2, R4)

Status: proposed, version 1. This is the UX of the Rust app. It builds on `DESIGN.md` (tokens, principles) and `UI_TOOLKIT.md` (components).

## What Kaia fixes in KOReader's UI

| KOReader today | Kaia |
|---|---|
| Settings spread over a 5-tab icon menu, many levels deep, with small rows | One Settings surface with plain sections; no setting deeper than two taps; search across settings |
| Reading options in a bottom panel of icon tabs with terse labels | One "Text and layout" sheet with labelled controls and a live page preview |
| Gestures are the main way to reach features, and they are invisible | Every feature has a visible control; gestures are shortcuts |
| File browser as the library | A library of books (covers, titles, progress) with sort, filter, collections and search; folders are one view among several |
| Dialogs with dense text and many buttons | Short sentences, at most three buttons, the main action on the right |
| Touch targets of 5 to 7 mm in many lists | Minimum 9 mm, rows of at least 14 mm |

The full KOReader feature set stays reachable. Where Kaia has no native control yet, Settings has an "Advanced" section with the remaining options.

## Global layout

- Bottom nav bar on Home, Library and Notes, plus the Settings button: Home, Library, Notes, Settings. Not shown while reading.
- Quick settings: swipe down from the top edge anywhere, or tap the status line. Frontlight slider, warmth slider, Wi-Fi toggle, rotation lock, and a Sleep button.
- Status line on Home, Library and Notes: time (updated only when the screen repaints for another reason, never on a timer), battery, Wi-Fi.
- Back: the Back key, a swipe from the left edge, and a back chevron in every header except the top-level surfaces.

## Home

As in `DESIGN.md`: the current book large with progress, chapter and time left; the next four recent books; a row with "Continue" for each book in progress if more than one; library counts. Empty state invites the user to add books (instructions for USB copy) and to import from KOReader when a KOReader folder exists.

## Library

- Views: Covers (grid, 3 columns portrait, 4 landscape), List (cover thumbnail, title, author, progress), Folders (the file system, for users who organize by folder).
- Sort sheet: title, author, recently read, recently added, progress, series. Filter chips: Reading, Not started, Finished, Collection, Format.
- Search field at the top: opens the keyboard; results update after each key with a 200 ms debounce; searches title, author, series and description through the library index.
- Long-press a book: Book details sheet (cover, metadata, description, progress, file info) with actions Open, Add to collection, Mark as finished, Remove from history, Delete file (confirm).
- Collections: a tab in Library, create, rename, reorder by drag on the list view.
- Paging, not scrolling: page buttons and swipe left or right turn library pages.

## Reader

- Full screen page. Tap zones (configurable, defaults): left third back, right two thirds forward, top strip opens the overlay, center opens the overlay. Page-turn keys turn pages. Swipe left or right turns pages.
- Overlay (tap center): top bar with back to Home, book title, search, bookmark toggle, and a "more" button; bottom bar with a page slider (chapter ticks), "page 42 of 311, 12 min left in chapter", and buttons Contents, Notes, Text and layout, Light.
- Contents panel: table of contents with the current chapter marked, bookmarks and highlights in tabs.
- Text and layout sheet (EPUB and other reflowable formats): font family list with previews, size stepper, line spacing stepper, margins stepper, justification toggle, hyphenation toggle, embedded styles toggle; the page behind the sheet updates live. For PDF and comics: zoom mode (fit width, fit page, columns), crop margins, contrast.
- Selection: long-press a word selects it; drag handles extend it. Selection popover: Highlight (with 4 colors on color panels and 4 styles on grayscale), Note, Sticker, Copy, Dictionary, Search in book, Share to Notes.
- Dictionary panel: offline StarDict dictionaries from the data folder; shows the definition in a sheet with a "more" button.
- Search in book: a field with results listed with context; tapping a result jumps there and highlights it.
- End of book: a sheet with rating, mark as finished, and the next book in the series if the library has it.

## Notes

As in the Lua app, rebuilt natively: books with annotations, newest first; per-book list in reading order; filters for highlights, notes, bookmarks, stickers; Needs repair group; export (Markdown, JSON, and KOReader's exporter formats) to the data folder.

## Settings

Sections, each a plain list:
- Reading: default font and size, tap zones, page-turn keys, page-turn animation off, refresh every N pages (E Ink full refresh), dictionary choice.
- Display and light: frontlight and warmth defaults, auto warmth by time, night mode, color rendering on or off, rotation.
- Library: book folders, scan now, cover style, show finished books.
- Network and sync: Wi-Fi networks, progress sync server (KOReader sync protocol), Calibre wireless, OPDS catalogs.
- Device: sleep and auto-suspend timers, sleep screen (book cover, image, or blank), power key behavior, USB storage, date and time.
- Extensions: native features and, after R7, KOReader plugins with their compatibility state.
- Data: import from KOReader, export notes, backup, safe mode.
- About: version, license, source, credits, KOReader attribution.
- Advanced: every remaining engine option, with a short explanation each.
- A search field at the top of Settings finds any setting by name.

## Sleep and power

- Sleep screen: the current book cover scaled to fill with a small "Kaia, sleeping" label, or a user image, or blank. Full refresh before sleep.
- Low battery warning at 10 percent; at 3 percent Kaia saves state and shows a "charge me" screen before sleeping.

## First run

A three-step welcome: language, where books are (default storage root), and import from KOReader if found. Then Home.

## Refresh policy

- Page turns: partial refresh; a full refresh every N pages (default 6) and after images.
- Opening sheets and dialogs: `ui` refresh of their area; closing: `ui` refresh of the uncovered area.
- Sliders while dragging: `fast` refresh; on release: `ui` refresh.
- Never refresh areas that did not change.
