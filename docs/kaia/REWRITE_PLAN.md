# Kaia Reader rewrite plan

Status: accepted by the project owner on 2026-10-13. This plan replaces the incremental migration in ENGINEERING_PLAN.md sections 3, 4 and 9 and ADR-001. The rest of ENGINEERING_PLAN.md (data protection, power, test and release rules, licensing) still applies.

## Goal

A Kobo e-reader written in Rust with a new UI, that reads what KOReader reads, keeps the user's data, and later runs KOReader plugins through a compatibility layer.

## Order of work

1. A clean Rust app with no Lua: platform, display, input, UI toolkit, document engines, reader, library, notes, settings.
2. Native Rust versions of the bundled KOReader features that matter most (statistics, sync, Calibre, OPDS and similar), with the new UI.
3. A Lua compatibility layer that embeds KOReader's LuaJIT and answers to KOReader's module names, so community plugins run. First target: the applicable bundled plugins plus a named, tested list of community plugins.

The KOReader Lua app stays in the repository as the reference for behavior and as the source for the compatibility layer until the Rust app reaches parity. The Kobo package ships the Lua app until the Rust app passes its gates.

## What stays C or C++

Document engines and panel drivers. Rust calls them through bindings and never reimplements them.

| Library | Use | Source |
|---|---|---|
| MuPDF | PDF, EPUB (first engine), CBZ, XPS, FB2, MOBI | KOReader's pinned build, `base/thirdparty/mupdf` |
| CREngine | EPUB and FB2 typography at KOReader quality | KOReader's pinned build, through a new C++ shim with a C ABI |
| DjVuLibre | DjVu | KOReader's pinned build |
| FreeType, HarfBuzz | Fonts and shaping for the UI and engines | KOReader's pinned build or Rust equivalents for the UI |
| FBInk | E Ink refresh on Kobo, including MTK panels | KOReader's pinned build |
| LuaJIT | Only in phase 3, for plugins | KOReader's pinned build |

## Compatibility seams (hard requirements from day one)

The Rust app must offer these from its first version, even with no Lua, so the plugin layer in phase 3 is translation and not a core rewrite. Each seam lists the KOReader behavior it has to support.

1. Named events. `Event { name, args: Vec<Value> }` with a dynamic `Value` type (nil, bool, number, string, table). `send` delivers to the top window first and stops when a handler returns true. `broadcast` delivers to every window. Order matches KOReader's `UIManager:sendEvent` and `broadcastEvent`.
2. Window stack. Ordered windows with modal flags, `show`, `close`, `top`, and `set_dirty(region, refresh_mode)`. Refresh modes map to KOReader's (`full`, `partial`, `ui`, `fast`, `a2`, `flashui`, `flashpartial`).
3. Scheduler. One main loop thread. `schedule_in(seconds, task)`, `next_tick(task)`, `unschedule(task)`. No async runtime.
4. Registries. Main menu entries with tabs and sorting hints; dispatcher actions (id, title, category, event name, argument); gesture bindings to actions.
5. Settings. A key-value store with `Value` entries, persisted as files Kaia owns, plus readers and writers for KOReader's Lua-table formats (`settings.reader.lua`, `.sdr/metadata.*.lua`) for migration and for plugins.
6. Document API. Page count, current location, page text, text and boxes for a selection, positions as page numbers or xpointers (CREngine), table of contents, links, search, and annotations with KOReader's field names (`pos0`, `pos1`, `page`, `text`, `note`, `chapter`, `datetime`).
7. Reader modules. The reader is a set of modules that register event handlers. A plugin in phase 3 becomes one more module.
8. Foreign widgets. A UI node that hands a pixmap region and input events to an external renderer. Phase 3 uses it to draw legacy Lua widgets that have no Kaia equivalent.

## Architecture

```text
kaia (binary)
  kaia-app        surfaces (Home, Library, Reader, Notes, Settings, Extensions), app state, navigation
  kaia-reader     reader modules: paging, TOC, highlights, bookmarks, search, dictionary, stickers
  kaia-ui         retained widget toolkit, layout, dirty regions, focus, gestures, theme, keyboard
  kaia-gfx        pixmaps (gray8, rgb), drawing, text shaping and layout, image decoding, dithering
  kaia-input      touch, keys, pen; gesture recognizer
  kaia-display    display trait; desktop window backend; Kobo backend through FBInk
  kaia-platform   battery, frontlight and warmth, suspend and resume, Wi-Fi, USB storage, paths
  kaia-doc        document trait; mupdf, cre, djvu backends (C bindings in kaia-doc-sys crates)
  kaia-events     Value, Event, window stack, scheduler, registries (the compatibility seams)
  kaia-luadata    KOReader Lua-table data reader and writer
  kaia-core, kaia-storage   existing: annotations and stickers (kept)
  kaia-lua        phase 3: LuaJIT embedding and the KOReader compatibility modules
```

`kaia-ffi` and the LuaJIT binding stay for the Lua app until it is retired.

## Testing without a device

Every surface renders headless to a PNG through the same toolkit code, so agents and CI check layouts with golden images. Gestures are replayed from scripts. Document backends are tested on the corpus from `docs/kaia/device/MEASUREMENT_METHOD.md`. Kobo builds use the existing toolchain image and the armv7 emulation smoke test. Device gates from ENGINEERING_PLAN.md section 8 still decide releases.

## Phases

| Phase | Deliverable | Exit check |
|---|---|---|
| R0 Foundations | kaia-events, kaia-gfx, kaia-input, kaia-ui core, desktop backend, headless PNG renderer, kaia-luadata | Toolkit renders a sample screen to PNG and a window on desktop; gesture replay tests pass |
| R1 Reader with MuPDF | Open EPUB, PDF, CBZ; page turn; TOC; progress saved; font size and margins | Corpus opens; page turn under 100 ms on desktop release build |
| R2 Shell | Home, Library (scan, covers, SQLite index, sort, filter, collections, search), Notes, Settings, on-screen keyboard, dialogs | Golden images for every surface; daily reading flow works on desktop |
| R3 Kobo | FBInk display, evdev input, frontlight and warmth, battery, suspend and resume, Wi-Fi, USB storage, launcher, package | armv7 build passes smoke tests; device test procedure written |
| R4 Annotations | Highlights, notes, bookmarks, stickers, in-book search, dictionary, export | Annotations survive reopen, reflow and crash tests |
| R5 CREngine | C++ shim and backend; hyphenation; KOReader typography settings | EPUB corpus renders like KOReader |
| R6 Native features | Statistics, progress sync, Calibre, OPDS, and other bundled features chosen from the inventory | Each feature has a spec and tests |
| R7 Lua layer | Embedded LuaJIT and compatibility modules | Bundled plugins and the named community list pass their workflows |
| R8 Migration and release | Import of KOReader and Kaia Lua-app data; Rust app becomes the default | Device gates pass |

## Rules

- Each crate has unit tests; UI work includes golden PNG tests.
- No `unsafe` outside `*-sys` crates, `kaia-display`'s Kobo backend, and the one `Lua::unsafe_new` call in `kaia-lua` (KOReader's Lua code needs LuaJIT's `ffi` and `debug` libraries, which mlua only enables through that constructor); every `unsafe` block has a comment stating the invariant.
- No network access in the product except features the user turns on.
- The writing style rules in CLAUDE.md apply to code comments, docs and UI strings.
