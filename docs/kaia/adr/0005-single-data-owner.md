# ADR-005: Single data owner, isolated profile, reversible migrations and durable exports

Status: Proposed

Date: 2026-10-13

## Context

Kaia must not run two independent copies of document state and keep them in sync with best-effort events. Each data class needs one owner. [ENGINEERING_PLAN.md section 3]

KOReader owns reading positions, document settings and legacy plugin data. Its sidecars hold the highlights and notes. The plan treats upstream DocSettings as the reference to characterize, not a format to replace. [S27]

New stickers need a store. The plan chooses SQLite for new Kaia-owned data. A database alone does not guarantee durability. SQLite documents its flush and filesystem assumptions, so the store must test disk-full, interrupted-write, unplug and failed-migration cases on the real storage. [S28]

Sticker anchors depend on the document text, not on a screen position. Stickers need a document identity and a re-anchoring path. [S30, S31]

Kobo USB mass-storage export changes the volume while a computer may be connected. Jobs must stop and handles must close before export. [S29]

## Decision

`docs/kaia/specs/STORAGE.md` assigns one owner to each data class.

- KOReader owns reading positions, document settings, highlights, notes and bookmarks through `DocSettings`. The Rust store does not write them and does not keep copies.
- The Rust store, `kaia-storage`, owns stickers and their image assets. It is the only writer for them.
- The library index is a derived cache. It can be rebuilt at any time and is never authoritative.

The store lives in `<DataStorage:getDataDir()>/kaia/`. The main database is `kaia.sqlite3`. It uses a rollback journal, `synchronous=FULL` and foreign keys. It has one connection and one writer. Each write is one transaction. Schema changes copy the database first, then run inside a transaction. A database with a newer schema version fails to open and stays unmodified.

Kaia uses its own app identity and settings directory. Test profiles are cloned from KOReader data. They are not shared with a live profile. Rust code never writes KOReader sidecars or settings. Kaia never modifies the Kobo account database.

Exports are versioned JSON documents with a copy of the asset files.

## Consequences

- The Notes view reads highlights and notes through KOReader. The Rust store cannot list them.
- Each schema migration needs disk space for its backup copy.
- Rolling back to an older executable does not undo a newer schema. An older build refuses to open the newer database. Recovery then needs a backup or the newer build.
- The rollback journal is the starting mode. The plan switches modes only when profiling justifies it.
- A plugin that writes private files or changes private fields bypasses the adapters. Kaia classifies such a plugin and tests it. Kaia then keeps the legacy owner or excludes the plugin.
- USB mass-storage export stops background jobs, flushes data and closes the database handle first.

## Evidence needed to accept

- The seven required fault tests in `docs/kaia/specs/STORAGE.md` pass. They cover reopen, a killed transaction, disk-full asset writes, a failed migration, a newer schema, export and import, and re-anchoring.
- KR-015 fixtures cover reflow, rotation, edited documents, orphan repair, power interruption, low disk space and app rollback compatibility.
- The data gate in ENGINEERING_PLAN.md section 8 holds: no lost committed annotations in the specified fault-injection tests.
- Disk-full, interrupted-write, unplug and restart, and failed-migration tests run on the real device storage.
- The Phase 4 exit gate requires the reopen, reflow, resize, rotation, document-change and interrupted-write tests to pass.

## Sources

- S27: Tagged document settings. https://raw.githubusercontent.com/koreader/koreader/v2026.07.1/frontend/docsettings.lua
- S28: SQLite durability assumptions. https://www.sqlite.org/atomiccommit.html
- S29: Tagged Kobo launcher. https://raw.githubusercontent.com/koreader/koreader/v2026.07.1/platform/kobo/koreader.sh
- S30: Tagged annotation handling. https://raw.githubusercontent.com/koreader/koreader/v2026.07.1/frontend/apps/reader/modules/readerhighlight.lua
- S31: Web Annotation selectors. https://www.w3.org/TR/annotation-model/
