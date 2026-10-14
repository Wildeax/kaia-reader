# Nickel parity plan

Status: plan, written 2026-10-14 and updated the same day after a static disassembly pass. Rust status in sections 2 and 4 was checked against commit 485aa0ec2 and the working tree on 2026-10-14. Lua status comes from the analyst reports and the KOReader code in this repository, with spot checks. Nothing here has run on a device.

## 1. Scope and sources

This plan covers Nickel, the stock reading software on the Kobo Libra Colour (codename monza), firmware 4.45.23697, kobo13 platform. It sets which Nickel features Kaia should match, in what order, and which ones stay out of scope. The target is the Rust app. The Lua app stays the shipped reference until the Rust app passes its gates (`docs/kaia/REWRITE_PLAN.md`).

What was studied. Exported symbols from the reader library, string tables from the stock binaries, SQL statements (table and column names only), class names, and file names from the unpacked root filesystem. A static pass then read the reader, platform and sensor code paths with Ghidra, which treated the firmware files as data. No firmware code ran, nothing was installed, and no network calls were made. Nothing was tested on a device.

Clean room. Each behavior is described in our own words. This repository holds no Nickel code, UI text, artwork or long strings. The static pass recorded behavior as statements, and this plan restates them. Addresses and decompiler output are left out. Nickel evidence appears only as class, method, key or table names, and only where a reader needs them to find the source of a claim.

Raw notes. Four analyst reports cover reading, library and home, device and system, and data, settings and formats. The static pass wrote a findings file of its own. Both stay outside the repository. This file keeps the merged conclusions and the identifiers needed to look things up again.

Terms. Lua means the shipped app: KOReader plus `frontend/kaia` and `plugins/kaia*`. Rust means `rust/kaia-app` and the crates it uses. Status is Yes, Partly or No, with a short note where the gap is not obvious. Value is how much closing the remaining gap matters to a Kaia reader: High, Medium or Low. A feature that both apps already cover rates Low.

Confidence. Confirmed means two sources agree, or a source states the behavior. Confirmed (from code) means the static pass established the behavior from control flow or constants. That does not mean a device test happened. Likely means one strong source fits the context. Guess means only a name points to it. Facts marked "from code" come from the static pass.

Sizes. S is one module or one screen, with tests. M is two to four modules or a new screen. L is a new subsystem or format, or work that needs device data first.

## 2. Feature matrix

Each row is one feature. Duplicates across the four reports are merged. Kobo-only services are in section 3.

### Reading

| Feature | What Nickel does | Lua | Rust | Value | Confidence |
|---|---|---|---|---|---|
| Page turns by tap, swipe and key | Taps, swipes and page keys turn pages. A layout choice sets which screen zones turn the page forward and back. | Yes | Partly. Tap zone presets, including left-handed. No key swap. | Medium | Confirmed |
| Page button inversion | Page keys map by orientation. In portrait they act as up and down, and in landscape as left and right. Inversion swaps each pair, and it is off by default (from code). The physical key identity is open (Q3). | Yes | Yes. A setting swaps each pair, and it is off by default. | High | Confirmed (from code) |
| Wake from sleep with a page button | A page button wakes the device after a hold time. The stock hold defaults to 0 ms where the device supports it (from code). Whether Monza has this support is open (Q12). | No | Partly. A setting exists, but no platform reports page button wake support yet, so it stays off. | Medium | Likely |
| Quick page flip | Holding a page key starts quick turn on the fifth repeat (from code). The inner zone steps every 600 ms, and outer zones step every 300 ms for text or every 150 ms for PDF and image-only pages (from code). A touch in the bottom strip also starts it. | No | Yes. A page key hold starts quick flip on the fifth repeat, at the stock intervals. | High | Confirmed (from code) |
| Taps, holds and swipes | Taps, double taps, long presses and swipes, each with a movement limit. The shared long-press interval is 350 ms (from code). | Yes | Partly. Taps and holds work. No double tap. | Medium | Confirmed |
| Pinch to change text size | Pinch changes the font size of text, and zooms images. | Yes | Yes. Pinch and spread change the text size of reflowable books. | Medium | Likely |
| Footer and header | Book and chapter footers default to page counts (from code). Choices are percent, time remaining, or pages. A chapter of one page or fewer shows no page count (from code). The header can show the chapter. | Yes | Partly. A footer line with a choice of items: page, percent, time left in chapter or book, clock and battery. No header item. | Medium | Confirmed (from code) |
| Time left estimate | Time left comes from word counts and a reading speed. The speed starts from a language default, English at 228 words per minute (from code). It learns from the reader's pace after at least 10 usable samples (from code). Under an hour the display shows whole minutes, and from an hour it shows hours with one decimal (from code). | Yes | Yes. Formula not compared. | Low | Confirmed (from code) |
| Font family | Lists system, bundled and user families. A publisher default keeps the book's own face. | Partly. No dyslexia faces. | Partly. Bundled faces, Atkinson Hyperlegible and OpenDyslexic, and user fonts. | Medium | Confirmed |
| Font size | A stepped size control, with a scale factor for large print. | Yes | Yes | Low | Confirmed |
| Dyslexia and low-vision faces | Ships faces for dyslexic and low-vision readers, and a dyslexic mode. | No | Partly. Atkinson Hyperlegible and OpenDyslexic faces. No dyslexic mode. | High | Confirmed |
| Font weight and sharpness | One slider from 0 to 120 steps sets both values. Weight is zero at step 40, and sharpness is zero at step 60 (from code). Weight runs from -0.20 to +0.80, and sharpness from -0.40 to +0.20 (from code). Values are saved per font family, and they are rendering adjustments, not CSS weights (from code). | Partly. Base weight only. | No | Medium | Confirmed (from code) |
| Line spacing, margins and justification | Scrubbers set line spacing and margins. Three justification choices. | Yes | Partly. No column setting. | Low | Confirmed |
| Hyphenation | Turns hyphenation on or off. Patterns ship for eight languages. | Yes. Patterns for many languages. | Partly. English only. | Medium | Confirmed |
| Publisher styles and embedded fonts | Keeps the book's styles and fonts, adds fonts for missing glyphs, and downloads script packs. | Yes. No script pack downloads. | Partly. Embedded styles on. No style tweaks. | Low | Likely |
| Dark reading mode and invert | Dark pages with light text, with a check for books that cannot use it. | Yes | Yes | Low | Confirmed |
| Footnotes, links and back | Taps follow internal links. Footnotes open in a preview. A back action returns to the earlier place. | Yes | Yes. Footnotes open in a popup, links jump, and a Back stack returns to the earlier place. | High | Confirmed |
| Highlight colors and selection | Long press selects a word. Highlights come in several colors, with shade and size. A menu offers highlight, note and lookups. | Yes | Partly. Hold selects a word. Colors and styles per highlight, from KOReader's palette. No shade choice. | High | Confirmed |
| Notes on highlights and margin marks | Notes are added or edited from the selection menu. Margin marks show where notes sit. | Yes | Yes. Note sheet, and margin marks on highlights with a note. | Medium | Confirmed |
| Export highlights | Writes highlights to a text file, sorted by location. | Yes | Partly. Notes export to Markdown and JSON. | Low | Confirmed |
| Bookmarks and dog-ears | A bookmark marks a page. A corner tap adds or removes a dog-ear. | Yes | Yes. Bookmark button and list. A tap in the top right corner bookmarks the page and shows a dog-ear. | Medium | Confirmed |
| Dictionary lookup | Shows an inline definition for a selected word. The panel switches dictionary and language, and keeps a My Words list. | Yes | Yes. Lookup in the chosen order, with each dictionary on or off, and a word list. | High | Confirmed |
| Dictionary packs and per-book dictionary | Downloads dictionary packs by language. Each book keeps its own dictionary. | Yes. Downloads from the KOReader list. | Partly. Reads StarDict folders. Each book keeps its own dictionary order. No downloads. | High | Confirmed |
| Translation | No machine translation of selected text. Bilingual dictionaries do that job. | Yes, with a network call (section 6.1) | No | Low | Likely |
| Table of contents | Lists chapters and jumps to one on tap. | Yes | Yes | Low | Confirmed |
| In-book search | Search with recent searches and suggestions. Hits are counted and highlighted. | Yes | Partly. Search and hits. No recent searches or suggestions. | Low | Confirmed |
| Go to page | Page number entry, chapter marks on the slider, and a back-to-page button after a jump. | Yes | Partly. Page entry and a back button after a jump. Percent slider and start and end jumps. | High | Confirmed |
| Fit and zoom for images | Fit width, height, page or half width. A zoom slider, panning and a magnifier. | Yes | Partly. Fit page, fit width and a zoom sheet. No half width. | Medium | Confirmed |
| Comic archives | Opens CBZ and CBR as comics. Full-screen image pages are detected. | Partly. CBZ through MuPDF. CBR not tested. | Partly. CBZ only. | Medium | Likely |
| PDF | Renders PDF with a separate engine. Annotations sit on their own layer. | Yes. Crop too. | Partly. Fixed pages with fit zoom. No crop. | Medium | Confirmed |
| Stylus ink and notebooks | Pen and eraser tools with color and size. Handwriting converts to text and math. Notebooks use templates. | No | No. Pen events exist, and nothing routes them. | Medium | Confirmed |
| Audiobooks and read-aloud | Plays audiobooks inside the reader. Read-aloud is an early-access item, and no speech engine turns up in the firmware. | No | No | Low | Confirmed |
| Reading statistics for a book | Hours read, pages per minute, average session length and time left. End matter can be left out. | Yes | Partly. Time per book. No per-book screen. | Medium | Confirmed |
| End-of-book prompt | Asks whether the book is finished on reaching the end. A choice can stop the prompt for that book. | Partly. End menu, no prompt. | Partly. End sheet with Mark as finished and Keep reading. | Medium | Confirmed |
| Delete a book when finished | A setting removes the local copy when a book is marked finished. | Partly. Delete from the end menu, after confirmation. | No | Low | Confirmed |
| Reading menu and quick access | A reading menu with font, zoom, stats and light buttons. A quick access panel opens by swipe. | Yes | Yes. Bars for Contents, Text and layout, Light and More. | Low | Confirmed |

### Library and home

| Feature | What Nickel does | Lua | Rust | Value | Confidence |
|---|---|---|---|---|---|
| Home screen tiles | Tiles for the current read, recent books, collections, authors, related reads and recommendations. Two switches show or hide sections. | Yes | Yes | Low | Confirmed |
| Continue reading | A tile for the book being read. | Yes | Yes | Low | Confirmed |
| Recently added tile | A tile lists new books. | No | Partly. Fallback card only. | Medium | Confirmed |
| Collection on home | One collection shows on home, chosen by when it was last opened. | No | No | Low | Confirmed |
| Remove from home | A flag hides a book from home lists. The book stays in the library. | Partly. Remove from history. | Partly. Clears the last open record, but the book can stay under Continue. | Low | Confirmed |
| Cover grid | Books, series and shelves as cover grids, with a fixed grid for each view. | Yes | Yes | Low | Confirmed |
| List view | One row per book, and a text table view. | Yes | Yes | Low | Confirmed |
| Sort by date added | Sorts by the date a book entered the library. | No. Sorts by modified date. | Yes. Recently added. | High | Confirmed |
| Sort by series | Sorts by series and series number. | Partly. Sort key exists. No series view. | Yes. Sort by series, and a series view. | High | Confirmed |
| Author sort and author view | Sorts by author. Opening an author lists that author's books. | Partly. Sort key only. | Yes. Authors tab, author pages, and more by the author on Home. | Medium | Confirmed |
| Filters and tabs | Unread, reading and finished filters, plus source and file type filters. The last tab is remembered. | Partly. Status filters only inside a collection. | Partly. Reading, Not started and Finished. No source or file type filter. | High | Confirmed |
| Library search | Search with recent searches, suggestions, a no-results state, and matches on keywords and series. | Partly. Collection search and file name search. | Partly. Full-text search and the last 10 searches. No suggestions. | High | Confirmed |
| Empty states and first run | Empty lists show a message. A first-run tour guides setup. | Partly | Partly. Empty library message. | Low | Confirmed |
| Folder browsing | Shows folders of sideloaded files and cloud folders. | Yes | Yes | Low | Confirmed |
| Collections | User lists called collections. Create, rename, delete, add and remove books. Names are unique. | Yes | Yes. Create, rename, delete, reorder, add and remove. | High | Confirmed |
| Series view and next book | A series list with covers. The next book comes from Nickel's server. | No. End of book opens the next file in folder order. | Yes. Series view, and the next book in series on Home. | High | Confirmed |
| Favorites | A favorite flag on each book, with its own list. | Yes | No | Low | Confirmed |
| Keywords and tags | Tags are shelves in the sync layer. Keywords are edited on the book. | Partly. Keyword editor. | No | Low | Likely |
| Book details | Synopsis, format, file size, language, publisher and date. Rating and similar items. | Partly. No publisher. | Partly. No publisher, rating or series. | Medium | Confirmed |
| Reading status | Unread, reading or finished. The status drives filters, sort and home. | Yes | Partly. Status chips. The end sheet can mark the book finished. | High | Confirmed |
| Mark many books finished or unread | Marks several books at once and reports the count. | Partly. One book at a time. | Yes. Select mode marks many books finished or unread. | High | Confirmed |
| Rating | A five-star rating that can be cleared. Reviews are kept on the account. | Yes. Rating and review in book info. | Partly. Rating stored. No screen. | Low | Confirmed |
| Delete a book | Removes the book, its images and its keys. | Yes | Yes. After confirmation. | Low | Confirmed |
| Archive a book | Removes the local copy and keeps the title in the account archive, where it can download again. | Partly. Moves the file to an archive folder by hand. | No | Medium | Confirmed |
| Storage by source | Shows space used by books, cloud sources, articles, previews and sketches, with delete all for each. | Partly. Cover cache and news downloads only. | Yes. Space by source (Calibre, OPDS and other books), with delete all for each. | Medium | Confirmed |
| Sideload import and rescan | Files copied to the device appear after a scan. Removed files drop out of the library. | Yes | Partly. Scan at start and on Scan now. No scan on USB unplug. | High | Confirmed |
| Dropbox books | Browse, download and sync book files. Exports go to Dropbox. | Yes | No | Medium | Confirmed |
| Google Drive books | The same model as Dropbox, with Drive folders. | No | No | Low | Confirmed |
| Library statistics | Books finished and unfinished, total hours, and the share of the library read. | Yes. Time, pages and books by week and month. | Partly. Reading time, book count, finished books and all-time reading. No share of the library read. | Medium | Confirmed |

### Device and system

#### Power and sleep

| Feature | What Nickel does | Lua | Rust | Value | Confidence |
|---|---|---|---|---|---|
| Sleep and wake with the power key | A short press toggles sleep on release (from code). A press held for 750 ms requests power off instead (from code). USB gadget mode and the cover state can block the toggle (from code). | Yes. Acts on release. | Yes. Sleep toggles on release, and a 750 ms hold powers off. | High | Confirmed (from code) |
| Power off by holding the power key | A hold of 750 ms requests power off (from code). The hardware forced-reset hold is a separate timing, and it is not measured. | Yes. Two seconds, and a release cancels. | Yes. A 750 ms hold powers off. | High | Confirmed (from code) |
| Auto sleep after idle | Choices are 5, 10, 15, 30, 45 and 60 minutes, or never. The default is 5 minutes (from code). | Yes. Default 15 minutes. | Yes. 1 to 120 minutes, default 15. | Low | Confirmed (from code) |
| Auto power off after idle | Choices are 5, 10, 15, 30, 45 and 60 minutes, or never. The default is never (from code). The timer likely uses a wake alarm. | Yes. Autosuspend, three-day default. | Yes. Off by default. Choices are 5 to 60 minutes, or never. | Medium | Confirmed (from code) |
| Sleep cover | On by default (from code). Closing the cover sleeps, and opening it resumes. The stock code ignores the cover in USB gadget mode (from code). | Yes | Yes. Works, with a setting. | Medium | Confirmed (from code) |
| Sleep screen | Shows the current book's cover by default, stretched to full screen on color devices (from code). The info panel is on by default, and it shows reading percent, time remaining and battery status (from code). | Partly. Screensaver types. | Partly. Cover or blank. | Low | Confirmed (from code) |
| PIN lock and unlock swipe | After a long sleep, a swipe-to-unlock screen. A four-digit PIN can be set. | No | No | Low | Confirmed |
| Suspend and resume | Checks the wake count before it sleeps, and turns Wi-Fi off first. | Yes | Yes | Low | Confirmed |
| Standby | The device classes have a standby method, and the UI has no standby setting. | Partly. Standby timer. | Partly. Plan exists, not called. | Low | Guess |
| Low battery warning | A dialog at low charge. The updater refuses to run at low charge. | No | Yes. A toast at 10%, and a sleep with the charge screen at 3%. | Medium | Confirmed |
| Battery percent | Battery percent and charging state. | Yes | Yes | Low | Confirmed |
| Charging indicator light | A setting turns the charging LED on or off. | Yes | Partly. LED write exists. No setting. | Low | Confirmed |
| Charger and USB plug events | Plug and charger events reach the UI. A plug can wake the screen. | Yes | Partly. Charging events reach the UI, and suspend is skipped while charging. | Low | Likely |

#### Light

| Feature | What Nickel does | Lua | Rust | Value | Confidence |
|---|---|---|---|---|---|
| Light key | The hardware light key toggles the frontlight. | Yes | Yes. The light key toggles the frontlight, with Nickel's fade times. | High | Likely |
| Frontlight level and popup | Brightness from 0 to 100. The saved level defaults to 80 on color devices and 30 on grayscale (from code). A popup holds brightness and natural light. | Yes | Yes. Quick settings sliders. | Low | Confirmed (from code) |
| Brightness swipe on the left edge | Starts in the leftmost 13% of the view width, and the first movement must be at least as vertical as horizontal (from code). Up brightens and down dims. A full-height drag covers the whole range. The level header stays for 2000 ms after the gesture (from code). On by default where the device has a light (from code). | Yes | Yes. A vertical swipe in the leftmost 13% of the page sets brightness, with a level header. | High | Confirmed (from code) |
| Natural light slider | Sets warmth from candle to white. | Yes | Yes. 0 to 100, restored at start. | Low | Likely |
| Natural light schedule | A fixed clock schedule, not sunrise or sunset (from code). Daytime holds from 07:00 to 18:00. From 18:00 the target moves in a straight line to bedtime, which defaults to 23:00 (from code). It holds until 05:00, then moves back to daytime by 07:00 (from code). Off by default on color devices (from code). | Partly. Autowarmth plugin, which can use sun position. Blocked for tests. | Yes. Nickel's day and bedtime schedule, off by default. | High | Confirmed (from code) |
| Ambient light sensor and auto brightness | Stock Libra Colour does not run it. Nickel limits ambient sensing to another device name, and Monza fails that check (from code). This is a code gate, not a hardware test. | No | No | Low | Confirmed (from code) |
| Light off in bright sunlight | Depends on the ambient sensor, which Nickel does not enable on Monza (from code). | No | No | Low | Confirmed (from code) |
| Light change ramp | Turning on fades to the saved level over 1500 ms. Turning off fades over 750 ms, or 5 ms when the change is immediate (from code). A fade cut short shortens in proportion to the distance left (from code). Popup and swipe changes apply at once (from code). | Yes | Yes | Low | Confirmed (from code) |

#### Display and e-ink

| Feature | What Nickel does | Lua | Rust | Value | Confidence |
|---|---|---|---|---|---|
| Full refresh every N page turns | Choices are 1 to 10 pages, and a chapter option. The stock default is chapter (from code). In chapter mode a new chapter resets the count when the count is above four (from code). Stopping quick turn, opening quick access or the footer menu, and finishing a pinch font-size change also force a clearing refresh (from code). | Yes. Six, with a separate night count. | Yes. Chapter by default, or every 1 to 10 pages. No night count. | Low | Confirmed (from code) |
| Waveform and dithering choices | Clearing refreshes use GC16 on gray regions and GCC16 on color (from code). Ordinary page changes use GLR16 or GLRC16 (from code). Dark mode uses GLKW16 on ordinary updates (from code). Update mode 1 applies when an update covers more than 85% of the screen (from code). | Partly. Eclipse waveform for night mode. | Partly. Fixed mapping. No toggles. | Low | Confirmed (from code) |
| Ghost clearing and VCOM check | A flash clears ghosting in upgrades. A warning shows when the stored board value does not match the panel. | Partly. Full refresh clears ghosts. | Partly. Full refresh with a flash. No VCOM check. | Low | Confirmed |
| Color rendering switch | Turns color post-processing for text on or off on the color panel. The final effect is open (Q21). | Partly. No switch found. | Yes. Setting for color rendering. | Low | Likely |

#### Buttons, orientation and screen

| Feature | What Nickel does | Lua | Rust | Value | Confidence |
|---|---|---|---|---|---|
| Orientation picker and lock | A rotate popup with auto, portrait and landscape. The lock is stored. | Yes. Lock and accelerometer toggles. | Partly. A lock and a picker for the four fixed orientations. No automatic choice. | High | Confirmed |
| Auto rotation | The orientation sensor turns the screen. | Yes. Accelerometer toggle. | No. Quick settings say automatic rotation is not available. | High | Likely |
| Water droplet detection | A switch detects droplets on the screen. With it off, droplets can turn pages. | No | No | Low | Likely |
| Screenshots | Saves the current screen to a file. A branch in the power key filter can reach it, and the setting that enables it is not identified (from code). | Yes | No. A development flag renders the app to PNG, and the device screen is not captured. | Low | Likely |
| Status bar icons | Icons for Wi-Fi, Bluetooth, rotation, light, sync and battery. | Partly. Battery on home and notes. | Partly. Text status line. | Low | Likely |

#### Connectivity and system

| Feature | What Nickel does | Lua | Rust | Value | Confidence |
|---|---|---|---|---|---|
| Wi-Fi prompts | Asks before a network action, and checks for captive portals. | Yes | Partly. Catalog screen only. | Medium | Confirmed |
| Airplane mode | One switch turns Wi-Fi and Bluetooth off. | Partly. Auto disable after idle. No switch. | Partly. A switch turns Wi-Fi off and keeps it off. No Bluetooth. | Medium | Confirmed |
| Bluetooth audio and remote | Pairing, audio output, and page keys from a Bluetooth remote. | No | No | Medium | Confirmed |
| USB storage and host plug | A computer plug starts mass storage, after an eject step. | Yes | Partly. Saves and returns to Nickel on a host plug. | Low | Confirmed |
| Date and time | Clock set from network time. Time zone from a server lookup or a city pick. | Yes. Network time plugin. | Partly. Time zone picker. No network time. | Medium | Confirmed |
| Language and keyboards | UI language and region, and keyboards for many languages. | Yes. Layouts for many languages. | Partly. UI language. One English keyboard. | Medium | Confirmed |
| Japanese and Chinese input engines | Input engines for Japanese, pinyin, Zhuyin, Cangjie and Korean. | Partly. Layouts only. | No | Medium | Confirmed |
| Firmware updates | Checks for updates, shows notes, installs, and refuses at low charge. | Partly. KOReader updates itself only. | No | Low | Confirmed |
| Factory reset | Erases books and settings. A boot path resets a damaged device. | No | No | Medium | Confirmed |
| Large print mode | A mode that scales text for low-vision readers. | Partly. Font size steps. | Partly. A setting scales the interface text by 1.25. | Medium | Confirmed |
| Read aloud and screen reader | Text to speech and a screen reader, listed as early-access items. | No | No | High | Confirmed |
| Developer options | A hidden page with tests, logs and feature flags. | Yes | No | Low | Confirmed |
| Beta and early-access flags | Named flags gate features for early access. | No | Partly. An experimental features page, with no flags yet. | Low | Confirmed |

### Data, settings, formats and extras

| Feature | What Nickel does | Lua | Rust | Value | Confidence |
|---|---|---|---|---|---|
| Import of Kobo reading data | Positions, highlights, collections, words, ratings and statistics live in the stock database. | No | Partly. Progress, status, highlights, notes, collections, words, ratings and sessions. No reading positions. | High | Confirmed |
| Annotation sync across devices | Highlights, notes and reading state sync through the account. The open route is in section 3. | No. KOSync covers progress only. | No. Progress sync only. | Medium | Confirmed |
| EPUB and KEPUB | Reads EPUB, and Kobo's KEPUB, which uses span markup. | Partly. KEPUB matched by suffix, untested. | Partly. KEPUB matched by suffix, untested. | High | Likely |
| FB2 and MOBI | Parses FB2. MOBI has a type, but no parser turned up. | Yes | Yes | Low | Likely |
| Plain text | Text files have a type, but no parser turned up. | Yes | Yes. .txt files are indexed and read with CREngine. | Medium | Guess |
| Image files and SVG | JPEG, PNG, GIF, WebP and SVG as pages or books. | Yes | No | Low | Likely |
| Offline Wikipedia | Reads ZIM archives through a helper program. | No. Online lookup only. | No | Low | Likely |
| Notebook export formats | Exports notes to Word, text, HTML, PDF and image archives. | Partly. Highlights exporter. | Partly. Markdown and JSON. | Medium | Confirmed |
| User font folder | Reads font files from a folder the user fills. | Yes | Yes. .ttf and .otf files in the fonts folder of the Kaia data folder. | Medium | Confirmed |
| Games | Four puzzle games. | No | No. Not planned. | Low | Confirmed |
| Sketch pad | A freehand sketch app, separate from notes. | No | No. Not planned. | Low | Confirmed |
| Web browser | A full browser with favorites. | No | No. Not planned. | Low | Confirmed |

### Checked and not present

- Nickel has no reading goal. Its goal values belong to the achievement rules.
- No cover size setting turned up. Each view has a fixed grid.
- No black-and-white cover mode turned up, and the dumps hold no color saturation or panel tuning controls.
- Nickel has no idle dimming. KOReader's autodim plugin does that, and the Rust app blocks it (section 6.4).
- Calibre has no Nickel counterpart. Kaia's Calibre support is a Kaia strength.
- The retail demo loop is not needed in Kaia.

## 3. Out of scope: Kobo-only services

These services depend on Kobo's accounts or servers. Kaia does not copy them. Where an open equivalent exists, the table names it.

| Kobo-only service | What Nickel does | Open equivalent | Decision |
|---|---|---|---|
| Kobo Store | Browses, previews and buys books, with storefront and reading lists. | OPDS catalogs. Kaia browses and downloads, with no purchase. | Out of scope. |
| Store previews and shortcovers | A sandbox reader for previews, and short companion reads attached to a book. | None | Out of scope. |
| Kobo Plus | Subscription books, offline access, and session records with start and end percent. | None | Out of scope. Do not copy the subscription. Session records appear in the import plan. |
| OverDrive loans and holds | Borrows, holds and returns library books with a library card. Includes the library finder. | OPDS feeds from the library, when it offers one. `rust/kaia-opds/src/opds1.rs` skips borrow links. | Out of scope for borrowing. OPDS browsing stays. |
| Discover lists | Featured lists from the store and from OverDrive. | OPDS sort relations, when a feed offers them. | Out of scope as a tab. |
| Next in series and related reads from the server | The server returns the next book and related reads. | Series data from the book files (T1-15). | Replace with the local rule. Do not call the server. |
| Recommendations and top picks | Server picks from setup answers and feedback. | None | Out of scope. |
| Wishlist | A store wishlist, synced to the account. | A collection | Out of scope. |
| Store reviews | Reviews from other readers, with helpful votes. | None | Out of scope. The local rating stays. |
| Kobo account sync | Library, reading state, shelves, ratings and achievements sync to the account. | KOSync for reading progress (`plugins/kosync.koplugin`, `rust/kaia-sync`). Calibre for libraries (`plugins/calibre.koplugin`, `rust/kaia-calibre`). WebDAV for files. | Out of scope. Progress sync only. |
| Annotation and notebook cloud sync | Highlights and notebooks sync to Kobo's cloud, with conflict choices. | Annotation export to a WebDAV folder, opt-in (T2-4). | Out of scope for the cloud. |
| Reading Life, achievements and activity cards | Badges, account statistics and home cards. Facebook sharing. | Local statistics (`rust/kaia-stats`, the statistics plugin) | Out of scope. No sharing. |
| Analytics and telemetry | Event uploads and analytics IDs. | None | Out of scope. `CLAUDE.md` forbids telemetry. |
| Parental content filters | A PIN that hides sensitive views, tied to the store's ratings. | None | Out of scope. The device PIN lock is in section 2. |
| Adobe DRM | An Adobe host opens DRM EPUB and handles license files. | None. DRM-free files only. | Out of scope. Upstream KOReader has no ADEPT support either. |
| Script font packs | Downloads fonts for other scripts from Kobo's server. | A user font folder (T2-5) | Replace with the user folder. |
| Dictionary packs from Kobo | Downloads dictionary packs from Kobo's server. | The KOReader dictionary list, as an opt-in download (T2-6) | Replace with the open list. |
| Pocket and Instapaper | Read-later services. Pocket is legacy. | Wallabag (`plugins/wallabag.koplugin`) | Out of scope. Do not build Pocket. |
| Firmware and backup services | Update checks and backup endpoints on Kobo's servers. | Kaia release packages (`docs/kaia/INSTALL_KOBO.md`) | Out of scope. |

## 4. Rust app roadmap

Tier 1 holds the high-value items at small or medium size, plus the device-feel basics. Tier 2 holds high-value items that need a larger build, plus the larger items the project owner named. Tier 3 holds the rest: nice-to-have items, medium or low value, and items blocked on a decision or a large build.

Three points changed after the static pass. Ambient light sensing moved from Tier 2 to Tier 3, because Nickel does not run it on the Libra Colour (from code), so stock parity does not need it. Several Tier 1 and Tier 2 rows now carry stock numbers, marked from code. Read aloud is high value, but it sits in Tier 3 because the firmware has no offline speech engine, so the engine is a decision first.

Items are numbered T1-1 to T1-21, T2-1 to T2-8 and T3-1 to T3-19. "Blocked by" names a question from section 7 (Q numbers) or a decision. "None" means the item can start now. Sizes follow section 1. The last column of each table starts with the status, checked on 2026-10-14. "Built" means the code has the feature, and any device check or open question follows it.

### Tier 1

| # | Item | Kaia files or crates | Size | Blocked by |
|---|---|---|---|---|
| T1-1 | Hardware light key toggles the frontlight. | `rust/kaia-display/src/kobo/decode.rs` (key 90 is dropped, lines 29 and 57 to 58), `rust/kaia-app/src/app.rs` (key handling), `rust/kaia-app/src/app/power.rs` (`toggle_light` exists, line 171) | S | Built. Still open: Q9 covers the light key flag on monza. Device check: the key sends code 90. |
| T1-2 | Brightness swipe on the left edge, with a level header. Starts in the leftmost 13% of the width. The first move must be vertical. Up brightens, and the header stays 2000 ms after the gesture (from code). | `rust/kaia-app/src/surfaces/reader/page.rs` (left edge is Back, lines 187 to 191), `rust/kaia-platform/src/frontlight.rs` | M | Built. Section 6.7, the left-edge decision, is still marked open. |
| T1-3 | Page button inversion, with a setting. Stock: page keys act as up and down in portrait, and as left and right in landscape. Inversion swaps each pair, and it is off by default (from code). | `rust/kaia-app/src/app.rs` (page keys), `rust/kaia-app/src/surfaces/settings/keys.rs`, `sections.rs` | S | Built. Q3 is a device check of which key sends which raw key. |
| T1-4 | Sleep on release, and a 750 ms hold powers off. Stock: a press held for 750 ms requests power off, and a short press toggles sleep on release (from code). Kaia's power-off constant is 2 s, copied from KOReader. Decide which value Kaia ships. | `rust/kaia-app/src/app.rs` (`Key::Power` acts on press, line 631), `rust/kaia-platform/src/events.rs` (`POWER_OFF_SECS` and `ArmPowerOff`, unused), `rust/kaia-app/src/app/power.rs` | M | Built, with 750 ms (`POWER_OFF_HOLD`), the stock value. Q5 covers only the hardware forced-reset hold, which Kaia does not need. |
| T1-5 | Sleep cover setting, on or off. Stock default is on, and the stock code ignores the cover in USB gadget mode (from code). | `rust/kaia-app/src/app.rs` (cover keys, lines 638 to 646), `keys.rs`, `sections.rs` | S | Built. Q11 does not block. |
| T1-6 | Auto power off after idle time. Choices are 5, 10, 15, 30, 45 and 60 minutes, or never. Stock default is never (from code). | `rust/kaia-app/src/app/power.rs` (idle check beside sleep, line 253), `keys.rs`, `sections.rs` | M | Built. T1-4 is done, so the power-off path is ready. Q6 does not block. |
| T1-7 | Wake from sleep with a page button. Stock default hold is 0 ms where supported (from code). | `rust/kaia-app/src/app.rs` (keys are ignored while asleep, line 646), `rust/kaia-platform/src/power.rs` (sysfs write on MT8113T), `keys.rs`, `sections.rs` | M | Partly built. The setting is in Settings, but no platform reports page button wake support yet, so it is off and disabled. Still open: Q9 and Q12 block the verified wake node. Device check: the page key wake nodes exist on monza. Q4 sets the side-effect rule. |
| T1-8 | Quick page flip with a page-key hold and a bottom-strip touch. Start on the fifth repeat. The inner zone steps every 600 ms. Outer zones step every 300 ms for text, or 150 ms for PDF and image-only pages (from code). Zones are fifths of the view width, and the center fifth pauses (from code). | `rust/kaia-app/src/surfaces/reader/page.rs`, `commands.rs` (hold, line 117), `overlay.rs` (slider), `rust/kaia-display` (skip dithering while flipping) | M | Built with the stock numbers. Q1 and Q2 still set the repeat timing and the strip height. A device test confirms the numbers. |
| T1-9 | Orientation lock and a manual picker. | `rust/kaia-app/src/surfaces/settings/quick.rs` (Fixed orientation row, line 64), `keys.rs` (`display.rotation_lock`, reserved), `rust/kaia-platform/src/device.rs` | M | Built. Auto rotation is T2-2. |
| T1-10 | Footnotes, internal links and back. | `rust/kaia-doc/src/types.rs` (`Link` and `LinkTarget`, lines 116 to 125), `rust/kaia-book` (link lookup), `rust/kaia-app/src/surfaces/reader/page.rs` and `commands.rs`, the reader state (back history) | M | Built. `readerlink.lua` and `readerback.lua` are the reference. |
| T1-11 | Go to page, and back to the page before a jump. | `rust/kaia-app/src/surfaces/reader/mod.rs` (`goto_percent`), `commands.rs`, `panels.rs` (page entry), `overlay.rs` (back button) | M | Built. |
| T1-12 | Highlight color choice, with shade. | `rust/kaia-app/src/surfaces/reader/mod.rs` (`highlight_color`, line 1207), `page.rs` (draw by color), `rust/kaia-book` (store the color with the annotation), `panels.rs` | M | Partly built. Color and style per highlight are in. Shade is not. Q29 sets the palette and shade range. |
| T1-13 | Dictionary order, per-book dictionary and a word list. | `rust/kaia-app/src/surfaces/reader/dict.rs` (reads `dictionary.order` and `dictionary.disabled`, lines 35 to 38, no writer), a settings screen, `rust/kaia-dict`, a word store in `rust/kaia-storage` | M | Built. Q32 sets how a book picks its dictionary. Not blocking. |
| T1-14 | Dyslexia and low-vision font faces. | `rust/kaia-gfx` (font loading), `rust/kaia-app/src/surfaces/settings/commands.rs` (family list, lines 112 to 115), `tools/kaia/package-kobo.sh` (ships the files) | M | Built. The check of each font's license is still open. The package ships both OFL texts. |
| T1-15 | Series order, a series view and the next book in series. | `rust/kaia-library` (series fields in `scan.rs`, sort in `query.rs`), `rust/kaia-app/src/surfaces/library/views.rs` (series view), `rust/kaia-app/src/surfaces/home/mod.rs` (next book card) | M | Built. Q43 sets how series numbers sort. Q44 sets the default. Not blocking. |
| T1-16 | Date added sort. The Rust sort menu already has it. Check the default and the label. | `rust/kaia-app/src/surfaces/library/views.rs` (Recently added, line 369), `data.rs` (`SortChoice::Added`) | S | Built. The sort menu has Recently added. The default sort stays Title. |
| T1-17 | Library search with recent searches and suggestions. Full-text search exists. | `rust/kaia-library/src/query.rs` (search), `rust/kaia-app/src/surfaces/library/mod.rs`, a recent-searches store in `rust/kaia-storage` | M | Partly built. Recent searches (last 10) are in. Suggestions are not. Full-text search was already there. |
| T1-18 | Rescan after USB unplug. | `rust/kaia-app/src/kobo/mod.rs` (the USB match has no unplug arm, line 122), `rust/kaia-app/src/app.rs` (`start_scan`, line 922), `rust/kaia-library/src/scan.rs` (missing rows kept for 30 days, line 27) | S | Built. Q40 and Q41 set Nickel's trigger and pruning rule. Decision still open: keep the 30-day retention, or drop removed files at once. |
| T1-19 | Remove a book from a collection. | `rust/kaia-app/src/surfaces/library/commands.rs` (new command), `rust/kaia-library/src/query.rs` (collection write) | S | Built. |
| T1-20 | Mark many books finished or unread at once. | `rust/kaia-app/src/surfaces/library/mod.rs` (selection), `rust/kaia-app/src/book_index.rs` (`mark_finished`, line 8), `commands.rs` (`MarkFinished`, line 63) | M | Built. |
| T1-21 | Finish prompt at the end of a book, with a still-reading answer. | `rust/kaia-app/src/surfaces/reader/panels.rs` (end of book sheet, lines 684 to 706), `rust/kaia-book` (status write), `book_index.rs` | M | Built. Q35 and Q36 set the rule. Not blocking. The rule is ours to set. |

### Tier 2

| # | Item | Kaia files or crates | Size | Blocked by |
|---|---|---|---|---|
| T2-1 | Natural light schedule on the stock clock times. Daytime target from 07:00 to 18:00. From 18:00 a straight-line move to the bedtime target, which defaults to 23:00. A hold until 05:00, then a straight-line move back to daytime by 07:00 (from code). Targets are 6400 (day) and 1900 (bedtime) in KOReader's Kelvin units. On Kaia's 0 to 100 warmth scale those are 0 and 92, using KOReader's linear conversion (from code). With the lamp on, a full 6400-to-1900 change fades for 60 seconds. The schedule recomputes on half-hour boundaries outside daytime (from code). Lua autowarmth can use sun position, but Nickel does not. | `rust/kaia-platform/src/frontlight.rs` (warmth writes, see section 6.9), `rust/kaia-app/src/services.rs` (warmth restored at start), a schedule module in `rust/kaia-app`, `keys.rs` | L | Built, off by default. Q10 sets the warmth node. Q14 sets a bedtime that falls inside the day window. The Lua reference is `plugins/autowarmth.koplugin`, blocked for tests (`PLUGIN_STATUS.md`, line 28). |
| T2-2 | Auto rotation from the orientation sensor. | `rust/kaia-platform/src/device.rs` (sensor path), a new orientation reader in `rust/kaia-platform`, `rust/kaia-app/src/app.rs` (redraw on turn), `quick.rs` | L | Q9 blocks, for the orientation sensor flag on monza. Needs T1-9 first. Device check of the sensor node. |
| T2-3 | Kobo data import: position, highlights, status, collections, words, ratings and sessions. | `rust/kaia-book` (position, highlights, status), `rust/kaia-library` (collections), `rust/kaia-stats` (sessions, beside `import_koreader`), `rust/kaia-app/src/surfaces/settings/section.rs` (import screen) | L | Partly built. Everything except reading positions is in. Positions are not imported and wait on Q33. Q34 and Q48 are device checks. Section 5 is the plan. |
| T2-4 | Annotation export to a WebDAV folder, opt-in only. | `rust/kaia-sync` or a new WebDAV module, `rust/kaia-app/src/surfaces/notes/export.rs`, `keys.rs` (network switch off by default) | L | Decision: the target and the format. Kobo cloud sync stays out of scope. |
| T2-5 | User fonts folder. | `rust/kaia-gfx` (font loading from a folder), `rust/kaia-book` (family list), `rust/kaia-app/src/surfaces/settings/commands.rs` | M | Built. User fonts load into CREngine. |
| T2-6 | Dictionary downloads, opt-in. | `rust/kaia-dict` (install and verify), `rust/kaia-app/src/surfaces/reader/dict.rs`, settings | L | Decision: source, checksum and an opt-in switch. The network rule applies. |
| T2-7 | Pinch to change text size. | `rust/kaia-input/src/gesture.rs` (`Pinch` exists), `rust/kaia-app/src/surfaces/reader/page.rs` (handler), `commands.rs` (size command), `keys.rs` (`reading.font_size`) | M | Built, with KOReader's step rule within 12 to 40. Q23 sets the size steps. Not blocking. The Lua reference is `readerfont.lua`. |
| T2-8 | Text weight and sharpness. Stock slider runs from 0 to 120 steps. Weight is zero at step 40, with steps of 0.005 below and 0.01 above. Sharpness is zero at step 60, with steps of 1/150 below and 1/300 above (from code). Values are saved per font family (from code). | `rust/kaia-book` (layout), `rust/kaia-gfx` (weight and sharpness in rendering), `panels.rs` (Font sheet) | L | Depends on the layout engine (`REWRITE_PLAN.md`, R5). Q26 blocks the sharpness part. |

### Tier 3

| # | Item | Kaia files or crates | Size | Blocked by |
|---|---|---|---|---|
| T3-1 | Stylus ink on pages, without handwriting recognition. | `rust/kaia-input/src/event.rs` (route `PenEvent`, line 96), a reader ink layer in `rust/kaia-app`, `rust/kaia-storage` (strokes), `rust/kaia-display` (ink refresh) | L | Q22 sets the ink refresh mode. Not blocking. Recognition needs an engine license, a separate decision. |
| T3-2 | Read aloud and a screen reader. High value. | A new speech module, a platform audio path, reader text access in `rust/kaia-book` | L | Q28 blocks. No offline speech engine turned up in the firmware. |
| T3-3 | Large print mode. | `rust/kaia-app/src/surfaces/settings/sections.rs` (mode switch), the reader font scale | S | Built. The scale is 1.25. |
| T3-4 | Japanese and Chinese input methods. | `rust/kaia-ui/src/controls/keyboard_layout.rs` (layouts), a new input engine crate | L | Decision: engine choice and license. |
| T3-5 | Experimental features switch, default off. | `rust/kaia-app/src/surfaces/settings` (one list of named flags) | S | Built. The page exists, but the flag list is empty, so no feature sits behind it yet. |
| T3-6 | Refresh cadence and waveform choices. Add the chapter option. Stock choices are 1 to 10 pages or chapter, with chapter as the default (from code). Kaia's stepper runs from 1 to 50 pages, so decide whether the default moves to chapter. Large image pages take the clearing path in stock (from code). | `rust/kaia-display/src/waveform.rs`, `rust/kaia-app/src/surfaces/settings`, `reading.full_refresh_pages` in `keys.rs` | M | Partly built. The chapter option is in, and chapter is the default. Still open: the waveform toggles (Q18), the image rule (Q17) and dither levels (Q19), with device tests. |
| T3-7 | Hyphenation for more languages. | `rust/kaia-book/src/layout.rs` (one dictionary, line 7), pattern files | M | Decision: pattern files and their license. |
| T3-8 | Footer and header item choice, margin marks for notes, and a corner dog-ear. Stock time format: whole minutes under an hour, and hours with one decimal from an hour (from code). With no statistics, stock estimates one minute per remaining page (from code). | `rust/kaia-app/src/surfaces/reader/overlay.rs` (footer), `mod.rs` (note marks), `page.rs` (corner tap) | M | Partly built. Footer item choice, margin marks and the corner dog-ear are in. The header item is not built. None blocks it. |
| T3-9 | Author view and author tile on home. Build with T1-15. | `rust/kaia-library/src/query.rs` (author index), `library/views.rs`, `home/mod.rs` | M | Built. Q44 sets the default sort. |
| T3-10 | Storage by source, with delete all. | `rust/kaia-app/src/surfaces/settings/sections.rs`, `rust/kaia-library` (size queries) | M | Built. |
| T3-11 | Finished count and totals in statistics. | `rust/kaia-stats`, `sections.rs` | S | Built. Q39 sets which event counts a book as finished. Not blocking. |
| T3-12 | Airplane mode switch. | `rust/kaia-platform/src/network.rs`, `wpa.rs`, `rust/kaia-app/src/surfaces/settings/quick.rs` | S | Built. |
| T3-13 | Time zone picker and network time. | `rust/kaia-platform/src/device.rs`, a time zone list, settings | M | Partly built. The time zone picker is in. Network time is not built, and it needs an opt-in switch. |
| T3-14 | Bluetooth page turner and audio. | A new Bluetooth module in `rust/kaia-platform` (BlueZ over D-Bus), `rust/kaia-input` (remote keys) | L | Device check of `mtkbtd` and the remote key codes. |
| T3-15 | CBR comics and plain text files. | `rust/kaia-doc/src/format.rs` (add CBR and text), `rust/kaia-library/src/scan.rs` (format gate) | M | Partly built. Text files are in. CBR waits on the decoder decision. |
| T3-16 | Notebook export to Word, PDF and image archives. | `rust/kaia-app/src/surfaces/notes/export.rs` | M | Decision: the formats and the PDF writer. |
| T3-17 | Calibre browse and search. | `rust/kaia-app/src/surfaces/catalogs` (Calibre view), `rust/kaia-calibre` | M | Built. Search and browse work on the Calibre metadata file the device downloads. |
| T3-18 | Factory reset path. | `rust/kaia-platform` (data wipe), `rust/kaia-app/src/surfaces/settings` | M | Decision: warning text, scope and recovery steps. `ENGINEERING_PLAN.md` safe mode rules apply. |
| T3-19 | Ambient light sensor and auto brightness, with the sunlight turn-off. Optional. Stock Libra Colour does not run it, because Nickel gates ambient sensing to another device name and Monza fails the gate (from code). Stock parity does not need it. The older sensor paths were written for that other device, so check each one before any use. | `rust/kaia-platform` (reader for the `als_*` sysfs nodes), `frontlight.rs` (ramp), `rust/kaia-app/src/app/power.rs` (poll), `keys.rs` | L | None for stock parity. Q13 and Q15 matter only if this item is built. |

## 5. Import plan for Nickel data

Goal. Carry a reader's Kobo state into Kaia without writing to Nickel's files and without copying account secrets. This plan is for the Rust importer (T2-3).

### Import status

The Rust importer covers steps 1, 2, 4, 5, 6, 7 and 8, plus progress percent
and status from step 3. Settings shows a plan with counts, asks for confirmation,
and reports imported and skipped records. It anchors highlights by text and
keeps notes with them. Words use the existing `kaia-storage` word list and keep
`DictSuffix` when present. Sessions keep their times and percentages in
`kaia-stats`, without guessed pages. Each owner stages its changes before file
replacement. Repeating an import skips records already stored.

Reading positions wait on Q33. The importer does not read
`ChapterIDBookmarked`, `ParagraphBookmarked` or `BookmarkWordOffset`. Step 9,
typography, is not included. Synthetic SQLite and EPUB tests cover this round.
A real Libra Colour database is still needed to check firmware column names,
KEPUB matching, repeated passages, session percent and idle-time units, and
snapshot consistency while Nickel is active. Q34 and Q48 remain device checks.

### Import rules


Safety. Copy `KoboReader.sqlite`, and any `-wal` and `-shm` files next to it, into a scratch folder. Open only the copy, read-only. Read columns by name, because the schema has had 176 migrations. Never write under `/mnt/onboard/.kobo`. Count rows per table in a plan step and show the counts before the import runs. Log counts per table, never titles, note text or words. Write results to a staging folder, then move them into place.

Steps.

1. Read the schema version from `DbVersion` and log it.
2. Map books from `content`. Resolve `adobe_location` to a file, matching by path first and then by title and author. Skip rows with no usable file.
3. Take the position from `ChapterIDBookmarked`, `ParagraphBookmarked` and `BookmarkWordOffset`. The meaning of these fields is open question Q33. Take progress from `___PercentRead` and the status from `ReadStatus`.
4. Take highlights and notes from `Bookmark`. Anchor each one by its text, because the container paths are DOM paths and do not survive reflow. Keep `Annotation` as the note. Skip rows with no text.
5. Make one collection per shelf in `Shelf` that is not deleted, with the books from `ShelfContent`.
6. Take words from `WordList`, with the book and the dictionary suffix (Q34).
7. Take the reader's own rating and review from `Ratings`.
8. Turn `KoboPlusReadingSessions` rows into per-book sessions, with start and end times and start and end percent.
9. Offer typography from `content_settings` and the last-used values as defaults the reader accepts. Low priority.

Owners. Position, highlights, notes, status, rating and time go to the kaia-book state files under `<data dir>/kaia/books/`, keyed by partial MD5. Collections go to `kaia-library`. Statistics go to `kaia-stats`, in a function beside `import_koreader`. The importer never writes KOReader sidecars or Nickel files. `docs/kaia/specs/STORAGE.md` decides the owner of each data class, and it needs an update before the importer ships.

Skip these tables. Secrets: `SecureUser`, `OverDriveUser`, `GDriveUser`, `content_keys`, and the Facebook token in `user`. Sync records: `DropboxItem` and `GDriveItem`. Analytics and experiments: `AnalyticsEvents`, `UsageData` and `AbTest`. Store and subscription data: `Entitlements`, `SubscriptionProducts`, the KoboPlus tables except sessions, `Wishlist`, `Reviews`, `Tab`, `volume_tabs`, `Achievement`, `Rules`, `Event` and `Activity`. The mail and contacts cache holds no reading data.

Test plan. Use a copy of a real `KoboReader.sqlite` from a Libra Colour, with sample EPUB and KEPUB files. The plan has not been run.

## 6. Repo issues

Line numbers match the working tree on 2026-10-14. Each issue was checked against the code before it was written.

### 6.1 Translate sends text to Google

Status. Open.

Evidence. `frontend/ui/translator.lua` sends selected text to a Google endpoint (line 297, and the header comment on line 4). `frontend/apps/reader/modules/readerhighlight.lua` registers the action as a highlight popup button (line 164), a long-press choice (line 370), and the translation settings and translate-page item (lines 877 to 881). The Wikipedia lookup is a long-press choice on line 371, and it also calls out over the network.

Recommendation. Remove the translate entries from the Kaia build. If translation stays, make it an explicit opt-in with a visible warning that the text leaves the device. Decide the Wikipedia lookup in the same change. The product rule allows network access only for features the user turns on (`docs/kaia/REWRITE_PLAN.md`, Rules).

### 6.2 Sync password stored as typed

Status. Open.

Evidence. `rust/kaia-app/src/surfaces/settings/keys.rs` line 20 documents `sync.password` as stored as typed, and line 51 defines the key. `rust/kaia-app/src/sync.rs` line 42 reads it and builds the credentials from it. KOReader keeps only a user key, an MD5 of the password. `plugins/kosync.koplugin/main.lua` hashes the password at lines 568 and 606, and saves only the key at lines 583 and 623. The Rust crate has a stored-key path, `Credentials::from_stored` in `rust/kaia-sync/src/model.rs` line 42, but only its tests call it.

Recommendation. After a login succeeds, save the user key through `from_stored`, and stop writing `sync.password`. The key still works as a login credential for that server, so the change limits exposure to other services that share the password. It does not make the file safe. The settings file sits in `/mnt/onboard/.kaia` (`rust/kaia-platform/src/paths.rs`), on the user partition that a computer sees in USB storage mode.

### 6.3 Lua app writes Nickel's Kobo config by default

Status. Open. Needs a decision.

Evidence. The flag `KOBO_SYNC_BRIGHTNESS_WITH_NICKEL` is set to true in `defaults.lua` line 188. That is the KOReader defaults file at the repository root. The analyst reports cite `frontend/kaia/defaults.lua`, but that file holds only `document_metadata_folder` (26 lines), so the reports point at the wrong file. `frontend/device/kobo/powerd.lua` lines 218 to 234 write frontlight state, level and color into Nickel's `Kobo eReader.conf` while the flag is on. No file under `docs/kaia` mentions the write, and nothing in `platform/kobo/kaia` changes the flag.

Intent. Nothing records it as intended. It is most likely inherited from KOReader, where the flag defaults to on.

Recommendation. Choose, and record the choice in `docs/kaia/specs/STORAGE.md`. The write keeps the stock reader's brightness in step, which helps when the reader goes back to Nickel. The Rust app writes no Nickel file, so the two apps differ. Unless the match is wanted, set the flag to false in `defaults.lua` line 188 with a comment. Keep the Nickel write behind an explicit setting.

### 6.4 Rust default plugin list omits autodim and autowarmth

Status. Open. Left out on purpose.

Evidence. `rust/kaia-app/src/plugins/mod.rs` lines 17 to 43 hold the DEFAULTS list, which the comment on line 17 describes as bundled plugins with a passing desktop workflow. `docs/kaia/inventory/PLUGIN_STATUS.md` marks both plugins as blocked (lines 24 and 28), and `docs/kaia/STATUS.md` line 120 lists them too. The analyst reports call this a defect. It is not one.

Recommendation. Leave the list as it is until the two workflow tests pass. `PLUGIN_STATUS.md` says the app now supplies frontlight and warmth readings and writes, so the missing piece is the scheduled workflow test. Write that test with T2-1, and use the stock clock times (18:00 to 05:00, bedtime 23:00, back to day by 07:00) as the expected values.

### 6.5 STATUS.md says the Rust app has no stickers

Status. Fixed on 2026-10-14. STATUS.md now matches the code.

Evidence. `docs/kaia/STATUS.md` line 27 says the Rust app has no stickers, and that only `kaia-ffi` links the storage crates. Line 133 says the app does not link the sticker crates. The code disagrees. `rust/kaia-app/src/surfaces/reader/stickers.rs` and `sticker_actions.rs` exist, `rust/kaia-app/src/surfaces/reader/mod.rs` lines 27 and 28 declare them, and `rust/kaia-app/Cargo.toml` line 29 depends on `kaia-storage`. Line 47 already says stickers are done on desktop.

Recommendation. Rewrite lines 27 and 133 to match line 47 and the code. The open item is a device test, not the code.

### 6.6 STATUS.md says kaia-calibre is not a dependency

Status. Fixed on 2026-10-14. STATUS.md now matches the code.

Evidence. `docs/kaia/STATUS.md` line 130 says `kaia-calibre` is not a dependency of `kaia-app`. `rust/kaia-app/Cargo.toml` line 23 lists it.

Recommendation. Correct line 130. The app has a Calibre view in its catalogs, and it still lacks search and browse (T3-17).

### 6.7 Left edge means Back in Rust and brightness in Lua

Status. Open.

Evidence. `rust/kaia-app/src/surfaces/reader/page.rs` line 18 sets the edge width to 40 dp, and lines 187 to 191 send a left-edge swipe to Back. `plugins/gestures.koplugin/defaults.lua` lines 14 and 15 map left-edge swipes to the frontlight, and lines 16 and 17 map right-edge swipes to warmth on devices with natural light.

Recommendation. Pick one meaning for the left edge before T1-2 starts. Nickel uses it for brightness. Move Back in the Rust reader to a corner tap or a bar button.

### 6.8 Collections import read the wrong path

Status. Fixed in commit fa9864613.

Evidence. Before that commit, the import read a top-level `collection.lua`. `rust/kaia-app/src/surfaces/settings/facts.rs` now picks the file through `collections_file` (lines 120 to 127), which prefers `settings/collection.lua` and falls back to the top-level file. A test covers both layouts (lines 203 to 217). The import call is in `rust/kaia-app/src/surfaces/settings/section.rs` line 265.

Next step. A device check with a stock KOReader folder would confirm the fix on hardware.

### 6.9 Frontlight color path is fixed to LM3630

Status. Selection rule in place. Device check open. `MixerNode::select` in `rust/kaia-platform/src/frontlight.rs` uses `/sys/class/leds/aw99703-bl_FL1/color` when that file exists, and `/sys/class/backlight/lm3630a_led/color` otherwise. The paths come from `frontend/device/kobo/device.lua` (line 439, and the runtime check at lines 747 to 752), because no file under `docs/kaia` records them. Nothing has run on a Libra Colour yet. On a device, run `tools/kaia/device/collect-profile.sh` and read `frontlight_aw99703_color_present` and `frontlight_lm3630_color_present`.

Evidence. `rust/kaia-platform/src/frontlight.rs` line 78 sets the Monza mixer to the `lm3630a_led` color node, and the module comment says the same. KOReader's KoboMonza table also names that node (`frontend/device/kobo/device.lua` line 548). The static pass found that Nickel's selection rule for Monza prefers an AW99703 color node when its control file exists, and falls back to LM3630 otherwise (from code).

Recommendation. Read the AW99703 color node on a Libra Colour before T2-1 ships. If that node exists, Kaia's warmth writes go to the wrong part, and the mixer needs the same selection rule. Add a test for each path.

## 7. Open questions for disassembly

Each row names the symbol or function to read, and the question to answer. The static pass answered some questions, and those are removed. Where it answered only part of a question, the row keeps the open part. Three questions were answered and removed, and two were merged into one. The numbers run from 1 to 57 in topic order. Questions 33 and 34 come from the import plan in the data report, not from the disassembly lists.

"Blocks" marks a question that stops a Tier 1, 2 or 3 item from being built correctly. A question that sets a value, a default or a rule does not block. Questions 9, 12, 26 and 33 block Tier 1 or Tier 2 items. Questions 18 and 28 block Tier 3 items.

### Page turns and quick turn

| Q | Read | Question | Roadmap effect |
|---|---|---|---|
| 1 | Input plugin that delivers the page keys, and `ReadingView::quickTurnInterval` overrides in derived readers | How soon the fifth repeat arrives on monza (the driver repeat delay and period), and whether any reader subclass overrides the interval. | Sets values for T1-8 |
| 2 | Constructor and call sites of `QuickTurnEventFilter` | Height of the bottom strip that accepts a quick-turn touch. | Sets values for T1-8 |
| 3 | `PageTurningOptionWidget::buttonsAndNames`, and the input plugin at the page-key boundary | Which physical key sends each raw page key on monza, and the full tap-zone geometry for each preset. | Sets rule for T1-3 |
| 4 | `PageTurnEventHandler::triggerSideEvents` | What a page turn triggers, such as a refresh, a wake or a timer. | Sets rule for T1-7 |

### Power, sleep and battery

| Q | Read | Question | Roadmap effect |
|---|---|---|---|
| 5 | `HardwareInterface::powerButtonActionDelay`, and the forced-reset path | The hardware forced-reset hold time, which is separate from the 750 ms software request. | None. T1-4 uses the software value. |
| 6 | Power code that resets the sleep and power-off timers | Which conditions reset those timers: charging, audio and background work. | None. The defaults are answered. |
| 7 | `LowPowerDialogController`, `N3PowerWorkflowManager::showBatteryLowView`, `UpgradeManager::lowBatteryAbort`, `MTK::pollBatteryLevelDelay` | Battery levels that trigger each low battery warning. | None |
| 8 | `MTK::suspend(int)`, `PowerManager::setDeepSuspend`, `HardwareInterface::suspendTimeInMilliseconds` | Which path a timed sleep takes on MTK. | None |
| 9 | `Device::calculateDeviceClasses`, `MTKMonzaDeviceConfig` | Values of `hasLightButton`, `hasOrientationSensor`, `hasPageTurnButtonWakeUp`, `supportsWaterDetection`, `hasStatusLED` and `isParentalControlEnabled` on monza. The ambient sensor flag is answered: off on monza. | Blocks T1-1, T1-7 and T2-2 |
| 10 | `FrontLight::FrontLight(QObject*)`, and the reader of the `FrontLight` config key | On monza, whether the AW99703 color control exists at run time, the warmth interval count the device reports, and the brightness step Qt applies to the slider. The selection rule is answered. | Sets values for T2-1 |
| 11 | `HallSensorFilter::filter` | What the 1000 ms timer started after a cover event does. The key behavior is answered. | None |
| 12 | Callers of `MTK::turnPageTurnWakeUpOn`, and `HardwareInterface::hasPageTurnButtonWakeUp` | Whether page-button wake is enabled on monza, and the condition for the sysfs write. | Blocks T1-7 |

### Light and natural light

| Q | Read | Question | Roadmap effect |
|---|---|---|---|
| 13 | `AmbientLightManager::turnOffAtSunlight`, `onLightTurningOff` | The delay before the sunlight shutoff. Needed only if T3-19 is built. | None for stock parity |
| 14 | `BedtimeManager::adjustTemperature` | How a bedtime that falls inside the daytime window is handled. The usual evening case is answered. | Sets rule for T2-1 |
| 15 | `KoboAmbientLightSensor::timerEvent` and any event-driven sample sources in `libksensors.so` | Event-driven sample sources, and the quantizer check. Needed only if T3-19 is built. | None for stock parity |

### Brightness and gestures

| Q | Read | Question | Roadmap effect |
|---|---|---|---|
| 16 | `QuinchGestureRecognizer::recognize` | What the second multi-touch gesture does. | None |

### Display and e-ink

| Q | Read | Question | Roadmap effect |
|---|---|---|---|
| 17 | `ReadingView::eventFilter`, and the PDF and image engine classes that the clearing rule skips | PDF and image engine exceptions to the clearing rule, the small-image DPI cutoff, and the exact visible page at chapter boundaries. | Sets rule for T3-6 |
| 18 | `KoboScreenMTK::handleAutoWfm`, `KoboScreenMTK::waveformModeFromQt`, `ReadingSettings::disableRegal`, `disableForcedGC16`, `toggleDisableDithering` | What the three toggles switch, the waveform for menu updates, and the full attribute-to-flag packing. | Blocks T3-6 |
| 19 | `PixmapSurface::getDitheringDepth`, `ReadingSettings::highQualityDithering` | The numerical dithering-depth policy. | Sets values for T3-6 |
| 20 | Writers of `FeatureSettings::invertScreen`, readers of `DARK_MODE` | How night mode reaches the panel on MTK. The dark-mode waveform is answered. | None |
| 21 | The setter behind the color rendering switch in `N3SettingsReadingView`, callers of `DevSettings::setCfaMode` | The final effect of reduce-rainbow mode, and what the CFA setting changes. | None |
| 22 | `SketchNote::setUpdateModeHeuristic` | Refresh mode for ink strokes. | Sets value for T3-1 |

### Text and typography

| Q | Read | Question | Roadmap effect |
|---|---|---|---|
| 23 | `ReadingMenuFontSettingsView::fontSizeScrubber`, `ReadingView::setCurrentFontSize` | Size steps and range. | Sets values for T2-7 |
| 24 | `ReadingSettings::lineHeightScalars`, `calcMinimumLineHeight` | Scale table and minimum line height. | None |
| 25 | `ReadingMenuFontSettingsView::marginsScrubber`, the margin container classes | Preset margin sizes in pixels. | None |
| 26 | `AdvancedFontSettingsController::saveSettings`, and the rendering code it feeds | How the renderer applies weight and sharpness (stroke expansion and sharpening). The slider mapping is answered. | Blocks the sharpness part of T2-8 |
| 27 | `ComicsReader`, `ComicsReaderParser` | Find the library that defines them. They are in neither symbol dump. | None |
| 28 | The feature flag for read aloud, then the shared libraries under the Kobo `usr/local` tree | Whether an offline speech engine ships. | Blocks T3-2 |

### Highlights, notes and stylus

| Q | Read | Question | Roadmap effect |
|---|---|---|---|
| 29 | `HighlightColor` enum, `MarkupTool::setOpacity` | Color count and shade range. | Sets values for T1-12 |
| 30 | `SelectionMenuController::setAutoHighlight` | When auto-highlight fires. | None |
| 31 | `StylusManager::stylusTipWear`, `tipConfig` | Tip-wear limit. | None for basic ink |

### Dictionary and words

| Q | Read | Question | Roadmap effect |
|---|---|---|---|
| 32 | `DictionaryParser::localeForDictionary`, `DictionaryParser::dictionaryForVolume` | How a book picks its dictionary. | Sets rule for T1-13 |
| 33 | Writers of `ChapterIDBookmarked`, `ParagraphBookmarked` and `BookmarkWordOffset` in the Content and Bookmark classes | How a reading position is stored: a chapter and paragraph index, or a DOM path. From the import plan. | Blocks T2-3 |
| 34 | Writer of `WordList.DictSuffix` | Which dictionary suffix a lookup records. From the import plan. | Sets rule for T2-3 |

### Reading status and finishing

| Q | Read | Question | Roadmap effect |
|---|---|---|---|
| 35 | `VolumeManager::updateBookmarkAndStatus`, `VolumeManager::updateReadingStatus`, `ReadingState::setLastTimeFinished`, `ReadingController::onFinishedBook` | Whether the first open or the last page sets the status, and what marks a book finished. | Sets rule for T1-21 |
| 36 | `VolumeManager::canShowEndOfBookDialog`, `FinishedReadingDialogController::maybeShow` | What decides when the end-of-book prompt appears. | Sets rule for T1-21 |
| 37 | `VolumeManager::removeBookOnFinish`, `ReadingSettings::getDeleteVolumeOnFinished` | Default value, callers, and whether the delete asks first. | None |
| 38 | `VolumeManager::archiveBooks`, `N3DeletionWizard::archiveBooks` | Which server request marks a title archived. | None |
| 39 | `GlobalStatsUtil::readingEventsUpdated`, `GlobalStatsUtil::totalBooksFinished` | Which event counts a book as finished. | Sets rule for T3-11 |

### Library scan and sideloading

| Q | Read | Question | Roadmap effect |
|---|---|---|---|
| 40 | `MassStorageManager::usbUnplugged`, `usbPlugged`, `sd_card_inserted`, `FSSyncManager::sync`, `SyncFileSystemCommand::execute` | Which event starts a library scan. | Sets rule for T1-18 |
| 41 | `SyncFileSystemCommand::findFilesToParse`, `SyncFileSystemCommand::pruneSideLoadedFiles` | Which folders are scanned, and what makes a file count as removed. | Sets rule for T1-18 |
| 42 | `EPubParser::parseMetadata`, `FB2Parser::parseTitleInfo`, `SyncFileSystemCommand::parsePDF`, `parsePlugin` | Which fields each format provides. | None |

### Library, series and home

| Q | Read | Question | Roadmap effect |
|---|---|---|---|
| 43 | The setter for `SeriesNumberFloat`, `EPubParser::parseMetadata` | How a series number such as 2.5 becomes a float. | Sets rule for T1-15 |
| 44 | Default sorters in `SeriesListBuilder`, `AuthorsListBuilder` and `LibrarySearchBuilder`; `LibraryViewMenuController::loadView` | Default sort of each list. | Sets defaults for T1-15 and T3-9 |
| 45 | `HomePageController::getPrioritizedWidgetList`, `configureWidgets`, `HomePageView::configureWidget` | Tile order, and the rule that picks the tiles. | None |
| 46 | `HomePageController::setTopRead`, `CurrentReadsFilter::filter` | How the top read is chosen. | None |

### Statistics and sessions

| Q | Read | Question | Roadmap effect |
|---|---|---|---|
| 47 | Fallback-book selection in `ReadingVelocityTracker::secondsPerWord`, and the reading stats code | The selection order for fallback books, the retention window of reading events, and the end-matter classification. The formula is answered. | None |
| 48 | `KoboPlusReadingSession::setIdleTime`, `ReadingView::readingSessionData` | How a session splits, and the idle cutoff. | Sets rule for T2-3 |

### Kobo services (out of scope)

| Q | Read | Question | Roadmap effect |
|---|---|---|---|
| 49 | `ActivityManager::enableActivities`, `makeTopPicksTabActivity`, `makeRecentBookActivity` | Rules for which home cards show. | None |
| 50 | `AchievementManager::save`, the writer of the Event table | Which reading events advance an achievement rule. | None |
| 51 | `ReadingState::priorityTimestamp`, `LibraryParser::parseChangedReadingState` | Which copy wins a sync conflict. | None |
| 52 | `OverDriveWorkflowManager::getExpiryStatus`, `haveItemExpiringSoon`, `haveExpiredItem` | Expiry windows for loans. | None |
| 53 | `GetNextInSeriesCommand::authenticatedExecute`, `VolumeTabManager::updateNextReadsVolumeTab` | What the server returns when a book has no next book. | None |
| 54 | `FrictionlessReadingCommand::populateQueue` and its callers | When the download queue fills. | None |
| 55 | `ParentalControlManager`, `HomePageView::toggleParentalControlSensitiveViews` | Which lists the PIN filters. | None |

### Developer and misc

| Q | Read | Question | Roadmap effect |
|---|---|---|---|
| 56 | Callers of `N3SettingsDeveloperOptionsController::loadView` | The route to the developer page. | None |
| 57 | The screenshot branch in `PowerKeyFilter::filter`, and callers of `ScreenshotMaker::saveCurrentScreen` | Which setting enables the screenshot branch, and the trigger. | None |
