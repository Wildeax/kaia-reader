# Kaia annotation storage contract (KR-015)

Status: proposed, version 1. This contract covers data that Kaia owns. Existing KOReader data (reading position, document settings, highlights in `.sdr` sidecars, plugin data) stays owned by KOReader code and is never written by the Rust store.

## Ownership

| Data | Owner | Rust store role |
|---|---|---|
| Stickers and their image assets | `kaia-storage` | Authoritative writer |
| Library index (title, author, cover path, last opened) | KOReader `ReadHistory` and `DocSettings` | Derived cache only, rebuildable, never authoritative |
| Highlights, notes, bookmarks | KOReader `DocSettings` (`annotations` table) | Not stored |
| Reading position and document settings | KOReader `DocSettings` | Not stored |

## Ownership in the Rust app (ADR-008)

The table above describes the Lua app, where KOReader code owns reading
state. In the Rust app, Kaia owns all reading state:

| Data | Owner in the Rust app | Location |
|---|---|---|
| Position, layout, highlights, notes, bookmarks, status, rating, reading time | `kaia-book` | `<data dir>/kaia/books/<partial_md5>.json` (`kaia_events::Settings` format, atomic writes) |
| Library index, covers, collections, history | `kaia-library` | `<data dir>/library.sqlite3` |
| Stickers and assets | `kaia-storage` | as below |
| Page ink strokes | `kaia-storage` | `<data dir>/kaia/kaia.sqlite3`, table `ink_strokes`, schema version 5 |
| Word list | `kaia-storage` | `<data dir>/kaia/kaia.sqlite3`, table `words`, see schema versions 3 and 4 |
| Reading statistics and imported Kobo sessions | `kaia-stats` | `<data dir>/statistics.sqlite3`, including `nickel_sessions` |
| App settings | `kaia-app` | `<data dir>/settings.json` |

KOReader sidecars (`.sdr/metadata.*.lua`, docsettings folders, `history.lua`, collections) are import sources only. Kaia reads them once when it has no state for a book and never writes them. Annotations keep KOReader's field names so the Lua plugin layer (R7) can present them to plugins.

The `layout` table of a book state holds `font_weight`, a KOReader font weight value (0 is regular, -1 to 3 in the Text and layout sheet). A missing key reads as 0.

## Kobo import

The Rust app offers an import when `<books dir>/.kobo/KoboReader.sqlite`
exists. It copies the database and any adjacent `-wal` and `-shm` files to a
private scratch folder. Only that copy is opened, with SQLite read-only flags.
The importer reads named columns from `DbVersion`, `content`, `Bookmark`,
`Shelf`, `ShelfContent`, `WordList`, `Ratings` and `KoboPlusReadingSessions`.
Missing optional columns or tables do not prevent other records from importing.
A missing schema version stops the plan. Views are refused. A SQLite authorizer
also blocks reads of tables outside this list, apart from SQLite schema metadata.

The plan shows the schema version, row counts for each source table and counts
for each kind of record before confirmation. Logs contain the schema version
and counts only. They contain no titles, notes, reviews or words.

Books match an indexed file by path first, then by a unique title and author
pair. Paths must resolve under the books folder, outside `.kobo`. A missing
or ambiguous file is skipped. `kaia-book` owns imported progress, status,
text-anchored highlights, notes, ratings, reviews and active reading seconds.
It writes its normal state file with KOReader annotation field names. Text
search picks the first matching passage. Missing text or a failed match skips
the highlight and its note. Container paths are not used. Position fields wait
for Q33 device checks. Typography is not imported.

`kaia-library` owns collections and their members, and caches book status and
percent. Deleted shelves and deleted memberships are skipped. `kaia-storage`
owns imported words in its existing word list, with an optional `dict_suffix`.
`kaia-stats` owns exact session start and end times, start and end percent, and
idle seconds in `nickel_sessions`. Valid sessions add active time to statistics,
without guessed page counts or reading positions. The book state also keeps
this time in `nickel_reading_seconds`, separate from page visits. Device checks
must confirm session field names, percent units and idle-time units for the
reader's firmware. Rows with missing or invalid bounds are skipped.

Every owner writes in a staging folder under the data directory first. The
importer then renames the closed files into place and reopens the library
connection. A lock excludes app library scans, and an open book prevents the
import. Target paths that resolve into `.kobo` are refused. Files are replaced
individually, not as one transaction across all owners. An interrupted publish
may leave some owners updated. Repeating the import fills the remaining owners.

Duplicate rules:

- Book state keeps `nickel_imports` ids for content, bookmarks and ratings.
  Each source id imports once per partial MD5. Later Kaia edits and annotation
  deletions survive another import. Unmatched text is not marked imported.
- Collection names match without regard to case. Existing memberships are
  kept, and only missing memberships are added.
- Words use the existing SQLite `NOCASE` unique key. An existing word keeps its
  book, context, timestamp and suffix.
- Sessions match by book identity, start time, end time and both percentages.
  The first matching session wins, including its idle time.

No queries read `SecureUser`, `OverDriveUser`, `GDriveUser`, `content_keys`,
`user`, `DropboxItem`, `GDriveItem`, `AnalyticsEvents`, `UsageData`, `AbTest`,
`Entitlements`, `SubscriptionProducts`, other KoboPlus tables, `Wishlist`,
`Reviews`, `Tab`, `volume_tabs`, `Achievement`, `Rules`, `Event` or `Activity`.
Mail and contacts caches are not read. The importer never writes Nickel files
or KOReader sidecars. Scratch copies, which can contain those tables as raw
SQLite pages, are removed when planning ends.

## Import from the Kaia Lua app

Settings, Data offers "Import from Kaia (Lua app)" when it finds Lua data.
The first screen shows counts. Import applies that preview, Cancel discards it,
and the report lists imported and skipped counts for each kind. Unsupported
annotation shapes appear as skipped "Other annotations" when present.

### Source paths

`frontend/kaia/identity.lua` names the Lua profile `kaia`.
`datastorage.lua` chooses its root in this order:

- `KO_HOME`, when set, overrides the platform paths.
- On Kobo, the launcher starts in `/mnt/onboard/.adds/kaia`, so the default
  data root `.` is the install folder.
- Desktop multiuser, AppImage and Flatpak builds use `$XDG_CONFIG_HOME/kaia`,
  or `~/.config/kaia` on Linux, or `~/Library/Application Support/kaia` on
  macOS. A desktop emulator without those modes uses its working directory.

The importer searches the override, the Kobo install under the books folder,
the desktop profile folders, the Rust install folder and the working directory.
A Lua profile in another emulator folder can be selected with `KO_HOME` when
starting Rust. It does not search stock KOReader folders for Kaia data.

| Lua data | Path relative to the Lua data root | Rust owner and destination |
|---|---|---|
| App settings | `settings.reader.lua` | Selected keys in `settings.json` |
| History | `history.lua` | `kaia-library`, `library.sqlite3` |
| Collections | `settings/collection.lua` | `kaia-library`, `library.sqlite3` |
| Position, layout, highlights, notes, bookmarks, status and rating | `docsettings/<absolute book path without its leading slash or extension>.sdr/metadata.<ext>.lua` | `kaia-book`, `kaia/books/<partial_md5>.json` |
| Hash sidecars, when selected | `hashdocsettings/<first two hash characters>/<hash>.sdr/metadata.<ext>.lua` | Same book state owner |
| Legacy book sidecars | `history/[<directory with slashes replaced by #>] <filename>.lua` | Same book state owner |
| Reading statistics | `settings/statistics.sqlite3` | `kaia-stats`, `statistics.sqlite3` |
| Stickers and saved dictionary words | `kaia/kaia.sqlite3` | `kaia-storage`, `kaia/kaia.sqlite3` |
| Sticker images | `kaia/assets/<sha256>.<ext>` | `kaia-storage`, `kaia/assets/` |
| Supported plugin settings | Selected tables in `settings.reader.lua` and files in `settings/` | Rust's Lua host data folder, as listed below |

`frontend/kaia/defaults.lua` sets `document_metadata_folder = "dir"`.
The import uses the Lua profile's docsettings, hashdocsettings and legacy history
folders. It does not fall back to a stock KOReader sidecar beside a book.
Paths from history, collections, the current library and the docsettings tree
identify books. Hash and legacy sidecars need a book in history, collections or
the library. Book files must still exist at their stored paths.

The Kobo stores are separate. Lua's store is
`/mnt/onboard/.adds/kaia/kaia/kaia.sqlite3`; Rust's store is
`/mnt/onboard/.kaia/kaia/kaia.sqlite3`. On desktop, Rust uses
`$XDG_DATA_HOME/kaia`, `~/.local/share/kaia` on Linux, or
`~/Library/Application Support/kaia` on macOS. The macOS profiles can therefore
share a store. When canonical database paths match, the importer reports the
stickers, assets and words as skipped and never opens that source store for writing.

### Settings mapping

Existing Rust keys win. Only valid values with a Rust consumer import.
Numeric values clamp to the bounds below. Sidecar layout continues to use the
existing `kaia-book` mapping and takes priority for that book.

| Lua key | Rust key | Conversion |
|---|---|---|
| `cre_font` | `reading.font_family` | Nonempty family name |
| `copt_font_size` | `reading.font_size` | 12 to 40 KOReader units |
| `copt_h_page_margins[1]` | `reading.margin_left` | 0 to 140 KOReader units |
| `copt_h_page_margins[2]` | `reading.margin_right` | 0 to 140 KOReader units |
| `copt_t_page_margin` | `reading.margin_top` | 0 to 140 KOReader units |
| `copt_b_page_margin` | `reading.margin_bottom` | 0 to 140 KOReader units |
| `copt_line_spacing` | `reading.line_spacing` | Clamp to 50 to 300, divide by 100 |
| `full_refresh_count` | `reading.full_refresh_pages` | 0 to 10, zero uses Rust's chapter refresh rule |
| `frontlight_intensity` | `display.frontlight` | 0 to 100, restored at startup |
| `frontlight_warmth` | `display.warmth` | 0 to 100, restored at startup |
| `input_invert_page_turn_keys` | `reading.invert_page_buttons` | Boolean |
| `highlight_drawing_style` | `reading.highlight_drawer` | `lighten`, `underscore`, `strikeout` or `invert` |
| `highlight_color` | `reading.highlight_color` | A color in `kaia-book::HIGHLIGHT_COLORS` |
| `dicts_order` | `dictionary.order` | Sort by numeric rank, resolve `.ifo` paths to StarDict book names |

The reader scales imported font and margin units to the screen width. Dictionary
order resolves dictionaries present under the Lua root's `data/dict`. Rust can
read that dictionary folder without copying or writing it. Unknown settings,
invalid values and dictionary entries without installed files do not become Rust
keys. The preview counts the mapped values, not every Lua setting.

### Supported plugin data

The Lua host receives these keys in its own `settings.reader.lua`. They retain
their Lua names and do not enter the native `settings.json` mapping:

- `statistics`, `readtimer`, `exporter`, `vocabulary_builder`, `httpinspector`
- `autoturn_timeout_seconds`, `autoturn_distance`, `profiles_autoexec`
- `calibre_wireless_url`, `calibre_wireless_password`,
  `calibre_wireless_default_format`, `inbox_dir`
- `language_japanese_text_scan_length`, `language_japanese_text_conversions`

Missing keys are added; existing keys win. A shared source settings file is left
alone. The importer also copies these files when the destination has no file:

- `calibre-extensions.lua`
- `settings/battery_stats.lua`, `settings/bookshortcuts.lua`
- `settings/cloudstorage.lua`, `settings/directory_defaults.lua`
- `settings/gestures.lua`, `settings/multiswipes.lua`
- `settings/kosync.lua`, `settings/kosync_queue.lua`
- `settings/move_to_archive_settings.lua`, `settings/news_settings.lua`
- `settings/opds.lua`, `settings/perception_expander.lua`
- `settings/profiles.lua`, `settings/text_editor.lua`
- `settings/vocabulary_builder.sqlite3`
- `settings/wallabag.lua`

Lua files must parse as data tables before confirmation. Statistics page records
use the native statistics importer. Saved dictionary words keep their spelling,
book, context, time and dictionary suffix in `kaia-storage`. Plugin data counts
one per settings key or file. Plugin code, arbitrary plugin files, caches and
credentials outside those supported settings are not copied. Imported plugin
settings take effect after a restart; the import does not enable plugins. The
legacy append log `settings/lookup_history.lua` is not a Lua-table file and is
not imported. Opening it in the Lua app first migrates its words into the
supported vocabulary database.

### Read and duplicate rules

Planning copies source data to a private temporary directory. SQLite databases
include adjacent WAL and SHM files. Only the copies are opened and checkpointed.
The plan runs the existing importers against temporary empty owners to count
records, then retains the same source snapshot until Import or Cancel. Neither
planning nor confirmation opens a source database with SQLite. Close the Lua app
before planning so multiple copied files describe the same saved session.

History and collections reuse `kaia-library`'s KOReader import. Statistics reuse
`Stats::import_koreader`. Book state reuses the sidecar candidate lookup, parser,
annotation mapping and atomic state writer in `kaia-book`. Stickers and assets
use the existing `Store::export` on the private copy and `Store::import` on the
destination, including hash and image checks.

- History matches a canonical book path and open timestamp. Missing files and
  unsupported formats are skipped. Existing last-opened times only move forward.
- Collections match names without case and add missing book memberships. Counts
  describe memberships, not collection names. Empty named collections still import.
- Book state imports only when neither its JSON file nor its backup exists.
  Repeats preserve later progress, annotation edits and annotation deletions.
  Import holds the normal closed-book lease and checks the file identity again.
  A new state also updates derived library status and progress without a new visit.
- Statistics match books by title, author and MD5, and page records by book, page
  and start time. Existing page durations win.
- Stickers match their UUIDs. Assets match their content hashes. Existing stickers
  and assets win, following the sticker transfer format's rules.
- Saved words match the SQLite case-insensitive spelling key. Existing context,
  timestamps and dictionary suffixes win.
- Settings fill missing Rust keys and keep a `lua_import.<key>` marker. Deleting
  an imported setting does not make the next import restore it.
- Plugin keys and files fill missing destinations only. Repeating the same
  import adds no keys or files. Existing plugin edits win.

The import refuses target symbolic links and shares the library scan lock with
the Kobo importer. Each owner commits independently. A failure can leave some
owners imported; retry fills the rest under these duplicate rules. No source
file is removed or renamed. Source-file bytes and modification times, including
live WAL files, are covered by the fixture tests. Shared desktop stores and the
headless preview, cancel and report screens have separate tests.

Device checks still need to confirm source discovery in the shipped Kobo package,
CREngine positions and annotation anchors from real Lua sidecars, font availability,
frontlight and warmth values, page key inversion, dictionary order, and plugin
settings after restart. The preview copies assets and uses temporary databases;
measure free-space needs and import time on a device-sized library.

## Location

The store lives in `<DataStorage:getDataDir()>/kaia/`:

```text
kaia/
  kaia.sqlite3          main database
  assets/<sha256>.<ext> sticker images, named by content hash
  backups/              database copies taken before each schema migration
  exports/              JSON exports written on request
```

## Document identity

A document is identified by `DocumentId`:

- `partial_md5` (string, 32 lowercase hex chars). This is KOReader's `util.partialMD5`, which the reader already stores as `partial_md5_checksum` in the sidecar. It is the primary key.
- `file_size` (u64, bytes). Used to detect a changed file that happens to keep its partial hash.
- `title` (optional string). Display only, never used for matching.

When a book is opened and its `partial_md5` matches but `file_size` differs, stickers for that document are flagged `needs_review` instead of being drawn at old positions.

## Sticker model

```text
Sticker {
  id:          string, UUID v4, lowercase, assigned by the store on insert
  doc:         DocumentId
  anchor:      Anchor
  asset:       string sha256 of an AssetRef, or null (null for text-only stickers)
  text:        string or null        (caption or text sticker, max 2000 chars)
  size:        { w: f32, h: f32 }    fraction of page width and height, each in (0, 1]
  rotation:    i16 degrees, -180..180
  state:       "attached" | "needs_review" | "orphaned"
  created_at:  i64 unix seconds
  updated_at:  i64 unix seconds
}
```

### Anchor

Exactly one variant:

```text
Reflowable {                      # EPUB, FB2, HTML and other CREngine formats
  locator:   string               # CREngine xpointer, stored verbatim, never parsed as CFI
  quote:     { exact: string, prefix: string, suffix: string }
  offset:    { dx: f32, dy: f32 } # placement relative to the anchor rect, in units of the rect height
  placement: "margin" | "inline"  # default "margin"; see below
}
Fixed {                           # PDF, DjVu, CBZ and other paged formats
  page:      u32                  # 1-based page number
  x, y:      f32                  # top-left in unrotated, uncropped page space, as fractions 0..1
}
Book {}                           # attached to the book, shown on the book's notes view
```

`placement` says where the reader draws the image. `margin` (the default) puts it in the page margin beside the anchor line. `inline` draws it over the text. The field lives inside `anchor_json`, so it needs no SQL migration. JSON without the field reads as `margin`. The store keeps the value as written and does not check it against the layout.

Limits enforced by `kaia-core` validation: `exact` 1..=1000 chars, `prefix` and `suffix` 0..=64 chars each, all floats finite, fractions in [0, 1], `dx` and `dy` in [-50, 50].

### Re-anchoring

For a `Reflowable` sticker, the reader first resolves `locator`. If the engine rejects it, or the text at the locator does not start with `quote.exact`, the reader calls `kaia_quote_locate` with the text of the current chapter. The function returns the best match:

1. An exact match of `prefix + exact + suffix` wins.
2. Otherwise an exact match of `exact` whose surrounding text has the most matching characters with `prefix` and `suffix` wins. Ties go to the earliest match.
3. Otherwise the sticker becomes `orphaned`. The store keeps it and the Notes screen lists it with a "Repair" action. Nothing is deleted automatically.

A font, margin or rotation change never changes stored data. Stickers are positioned from the anchor at draw time.

## Asset rules

- Accepted types: PNG and JPEG, detected from magic bytes, not the file name.
- Maximum encoded size 4 MiB. Maximum decoded size 2048 x 2048 pixels, read from the header before decoding.
- The file is named by its SHA-256 and written to `assets/` with write to temp file, fsync, rename.
- `AssetRef { sha256, mime, width, height, bytes, source_name, license }`. `license` is free text the user may leave empty.
- An asset is pending from each add until a sticker saves a reference to it. Adding the same image again makes it pending again.
- `kaia_store_gc` removes an asset that no sticker references. A pending asset stays until 60 minutes (`PENDING_ASSET_GRACE_SECS`) have passed since its last add. gc never runs during ordinary writes.
- If gc runs after the 60 minutes and before the sticker is saved, the image is removed and the save fails with `unknown asset`. Adding the image again fixes it.
- After a crash between an add and its save, the next gc that runs more than 60 minutes after the add removes the image.
- Asset files with no row, such as leftovers from an interrupted write, are removed at once.
- A pending time later than the clock, which happens when the clock moves back after an add, keeps the asset until the clock passes that time plus 60 minutes.

## Database rules

- SQLite via `rusqlite` with the bundled SQLite.
- `journal_mode=DELETE` (rollback journal), `synchronous=FULL`, `foreign_keys=ON`.
- One connection per open store. One writer. Every write is a single transaction.
- Schema version in `PRAGMA user_version`. Migrations run in order inside one transaction each. Before any migration the store copies the database to `backups/kaia-v<old>-<unix time>.sqlite3` using the SQLite backup API.
- Opening a database whose `user_version` is newer than the code supports fails with `KAIA_E_SCHEMA_TOO_NEW` and does not modify the file. This keeps an older executable from damaging newer data after an app rollback.

### Schema version 1

```sql
CREATE TABLE documents (
  partial_md5 TEXT PRIMARY KEY,
  file_size   INTEGER NOT NULL,
  title       TEXT
);
CREATE TABLE assets (
  sha256      TEXT PRIMARY KEY,
  mime        TEXT NOT NULL,
  width       INTEGER NOT NULL,
  height      INTEGER NOT NULL,
  bytes       INTEGER NOT NULL,
  source_name TEXT,
  license     TEXT
);
CREATE TABLE stickers (
  id          TEXT PRIMARY KEY,
  doc         TEXT NOT NULL REFERENCES documents(partial_md5),
  anchor_json TEXT NOT NULL,
  asset       TEXT REFERENCES assets(sha256),
  text        TEXT,
  w REAL NOT NULL, h REAL NOT NULL,
  rotation    INTEGER NOT NULL DEFAULT 0,
  state       TEXT NOT NULL CHECK (state IN ('attached','needs_review','orphaned')),
  created_at  INTEGER NOT NULL,
  updated_at  INTEGER NOT NULL
);
CREATE INDEX stickers_doc ON stickers(doc);
CREATE TABLE library_cache (
  path        TEXT PRIMARY KEY,
  partial_md5 TEXT,
  title       TEXT,
  authors     TEXT,
  last_opened INTEGER,
  percent     REAL
);
```

`library_cache` may be dropped and rebuilt at any time.

### Schema version 2

```sql
ALTER TABLE assets ADD COLUMN pending_since INTEGER;
```

`pending_since` holds the unix time of the last add of an asset, if no sticker has saved that asset since. Otherwise it is NULL, and so is every row that existed before version 2. Exports do not carry it, and an import leaves the assets it adds not pending.

### Schema version 3

```sql
CREATE TABLE words (
  id          INTEGER PRIMARY KEY,
  word        TEXT NOT NULL UNIQUE COLLATE NOCASE,
  book_title  TEXT,
  context     TEXT,
  created_at  INTEGER NOT NULL
);
CREATE INDEX words_created ON words(created_at);
```

`words` is the word list. It holds the words the reader saved from dictionary lookups. `word` is unique ignoring case. Saving a word again keeps its `id` and replaces `book_title`, `context` and `created_at`. `created_at` holds the Unix time of the last save. `context` is the sentence around the word. The store clips `book_title` to 300 characters and `context` to 1000. It refuses a word that is empty or longer than 100 characters. The list reads newest first, by `created_at` and then `id`. The sticker export does not carry the word list.

### Schema version 4

```sql
ALTER TABLE words ADD COLUMN dict_suffix TEXT;
```

`dict_suffix` keeps the Kobo dictionary suffix when supplied. The importer
clips it to 100 characters. Older words have NULL. Native word saves retain an
existing suffix. Q34 does not block importing a supplied suffix as text.

### Schema version 5

```sql
CREATE TABLE ink_strokes (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  doc TEXT NOT NULL REFERENCES documents(partial_md5),
  page INTEGER NOT NULL CHECK (page >= 0),
  locator TEXT,
  points_json TEXT NOT NULL,
  width REAL NOT NULL CHECK (width > 0),
  color TEXT NOT NULL
);
CREATE INDEX ink_doc_page ON ink_strokes(doc, page);
```

`kaia-storage` is the only owner of page ink. The reader never writes ink to
KOReader sidecars. Each row is one complete stroke. `page` is the zero-based
page number at creation. `points_json` is an array of `{x, y, pressure}` objects.
Pressure is the raw device value. Brush widths are constant for now, with two
choices in the reader. Colors are `black`, `blue`, `green`, and `red`; colored
ink uses the highlight palette and displays as black on grayscale panels.

Fixed-layout books use unrotated document page coordinates and a NULL locator.
The reader applies the page's scale and centering at draw time. Width is also in
page units, so ink follows PDF and CBZ zoom changes.

Reflowable books use the page's text xpointer as `locator`. Points are offsets
from its first character box in layout pixels. On a later visit, the reader
resolves that xpointer to its new page and character box, then applies the saved
offsets. A font or margin change does not rewrite the stroke. Ink can drift away
from the words after a relayout because the stroke keeps its original shape.
Unresolved anchors stay in storage but do not draw. Reflowable engines without
text positions cannot accept ink.

The store validates finite coordinates and widths, supported colors, a maximum
of 100,000 points per stroke, and a maximum locator length of 16,384 bytes. A
stroke commits on pen lift. An interrupted stroke is not saved. Erasing removes
whole strokes in one transaction on lift. Undo removes the latest remaining
stroke on the visible page. Delete all removes the visible page's strokes after
confirmation. Undo does not restore erased or deleted ink.

Notes export includes `ink_counts`, an array of `{page, count}` objects per book,
and a count per page in Markdown. Export page numbers start at one and refer to
the original layout. Counts can therefore differ from the current page numbers
after a relayout. The sticker transfer format does not include ink.

## Export format

`kaia_export` writes `exports/kaia-export-<unix time>/` containing `stickers.json` and an `assets/` copy:

```json
{
  "format": "kaia-stickers",
  "version": 1,
  "exported_at": 1760000000,
  "app_version": "kaia-0.1.0",
  "documents": [ { "partial_md5": "...", "file_size": 123, "title": "..." } ],
  "assets":    [ { "sha256": "...", "mime": "image/png", "width": 64, "height": 64, "bytes": 2048, "source_name": "star.png", "license": "" } ],
  "stickers":  [ { "id": "...", "doc": "...", "anchor": { "type": "reflowable", "locator": "...", "quote": { "exact": "...", "prefix": "...", "suffix": "..." }, "offset": { "dx": 0, "dy": -1.2 }, "placement": "margin" }, "asset": "...", "text": null, "size": { "w": 0.1, "h": 0.07 }, "rotation": 0, "state": "attached", "created_at": 0, "updated_at": 0 } ]
}
```

`kaia_import` accepts this format, skips stickers whose `id` already exists, and reports counts. Unknown top-level fields are ignored. An unknown `version` greater than 1 is rejected.

## Required fault tests

The test suite must cover each of these with a fixture:

1. Reopen after a clean write returns identical stickers.
2. A process killed between `BEGIN` and `COMMIT` (simulated by dropping the connection mid-transaction) leaves the previous committed state.
3. A disk-full error during an asset write leaves no partial file in `assets/` and no sticker row.
4. A failed migration rolls back and leaves the original file and `user_version` unchanged, and the backup exists.
5. Opening a database with a newer `user_version` fails without writing.
6. Export followed by import into an empty store reproduces the same stickers and assets.
7. Re-anchoring finds the quote after text is inserted before it, and orphans the sticker when the quote is removed.
