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
| App settings | `kaia-app` | `<data dir>/settings.json` |

KOReader sidecars (`.sdr/metadata.*.lua`, docsettings folders, `history.lua`, collections) are import sources only. Kaia reads them once when it has no state for a book and never writes them. Annotations keep KOReader's field names so the Lua plugin layer (R7) can present them to plugins.

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
