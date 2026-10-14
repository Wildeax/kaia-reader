# Community plugin desktop status

L5 part 2, repository snapshot checked on 2026-10-13. Desktop adapters updated
on 2026-10-14. Nine plugins are pinned. Seven work with limits and two are
blocked. No plugin has a complete Kobo device workflow claim. A passing blocked test confirms a specific missing prerequisite,
not a working main job. L5 remains incomplete.

## Selection and licenses

Selection used `gh search repos "koreader plugin" --sort stars --limit 50`,
`gh search repos "koreader annotations export" --sort stars --limit 10`, and
`gh api` repository, default-branch commit and pinned archive endpoints.
All nine repositories exist, are public and are not archived. Stars, forks and
commit dates below are the checked snapshot. They are evidence of use and
activity, not a promise of future maintenance. The KOReader wiki contents request
`gh api repos/koreader/koreader.wiki/contents/User-plugins.md` returned HTTP 404.
No wiki endorsement is claimed.

Both original URLs exist. Pencil and pagenotes stay on the list for stylus and
page-note workflows. `renandeivison/quicksettings` was excluded because its
checked tree has no license file or explicit license declaration.
`omer-faruq/readingstyle.koplugin` has GPL-3.0 terms, but was not selected because
Bookends already covers page UI changes. App stores and download managers were
not priorities under the product no-network rule.

License review used the pinned LICENSE files, plus explicit upstream notices.
Footcream's README grants AGPL-3.0-or-later. The other AGPL entries use the
upstream AGPL-3.0 label without inferring a later-version grant from the example
notice in the license appendix. These licenses meet the project's stated
redistribution criteria. No GPL-2.0-only or unlicensed plugin is vendored.

| Plugin | Repository | Pinned commit | License | Stars / forks | Latest commit | Reason |
|---|---|---|---|---|---|---|
| pencil.koplugin | [mysticknits/pencil.koplugin](https://github.com/mysticknits/pencil.koplugin) | `f4e58b315c972a3ff663380cf6e186c32bc339d2` | AGPL-3.0 | 158 / 26 | 2026-05-14 | Stylus handwriting made for the Kobo Libra Colour. Keeps the original candidate. |
| pagenotes.koplugin | [toastiez/pagenotes.koplugin](https://github.com/toastiez/pagenotes.koplugin) | `5876d17ad38af09dc12c20a3b30aead1b9bf579b` | MIT | 23 / 1 | 2026-09-13 | Page notes and stickers, including colour artwork. Keeps the original candidate. |
| footcream.koplugin | [Fank1/foot-cream](https://github.com/Fank1/foot-cream) | `9d4edc4c08664f133b7064ba2cb3a7991feb914f` | AGPL-3.0-or-later | 105 / 3 | 2026-09-08 | Converts unfamiliar measurements while reading. Provides a reading aid beyond timers and statistics. |
| anki.koplugin | [Ajatt-Tools/anki.koplugin](https://github.com/Ajatt-Tools/anki.koplugin) | `55b07ed93d58f2c578c81a28c6fa782aec31d866` | AGPL-3.0 | 235 / 27 | 2026-08-20 | Exports dictionary words as study cards and can queue notes offline. Has 27 forks. |
| highlightsync.koplugin | [gitalexcampos/highlightsync.koplugin](https://github.com/gitalexcampos/highlightsync.koplugin) | `bfaf711e2522a3f5e1b1a0e163e65095af1145c8` | AGPL-3.0 | 236 / 21 | 2026-02-12 | Merges annotations across devices. Has 236 stars and 21 forks, but its latest default-branch commit is older than the other candidates. |
| readingstreak.koplugin | [advokatb/readingstreak.koplugin](https://github.com/advokatb/readingstreak.koplugin) | `dea5f707c1f8e6c68d917727f6dc7cc6d121db5b` | AGPL-3.0 | 64 / 3 | 2026-09-09 | Tracks daily reading goals without a server. Adds a reading aid with persistent history. |
| bookends.koplugin | [AndyHazz/bookends.koplugin](https://github.com/AndyHazz/bookends.koplugin) | `217a15e4e1b6d5fad94a0cfa6bee303b81b698b5` | AGPL-3.0 | 500 / 21 | 2026-10-06 | Custom page text and progress overlays. The most popular selected plugin, with 500 stars and recent commits. |
| readeck.koplugin | [iceyear/readeck.koplugin](https://github.com/iceyear/readeck.koplugin) | `a1fba4a59b9731e451a63eea4e927c7c16198d7c` | MIT | 130 / 13 | 2026-05-07 | Read-later articles, annotation export and library sync. Has 130 stars and 13 forks. |
| foldermemory.koplugin | [Craftwork2720/foldermemory.koplugin](https://github.com/Craftwork2720/foldermemory.koplugin) | `022202df58b6c721653ccd5e8bbb5bd90d621341` | AGPL-3.0 | 18 / 0 | 2026-10-09 | Remembers sorting and display choices per library folder. A smaller candidate with an October commit. |

## Baselines and results

Layer tests live in `rust/kaia-lua/tests/community.rs`. They copy unchanged fixtures
into a scratch data folder and pass that folder through `PluginOptions.extra_paths`.
This also preserves the data/plugins layout that Anki and Bookends assume.
Reader tests use the existing BookSession host for `test/juliet.epub` or
`test/sample.pdf`. Folder Memory uses LibraryHost with a scratch copy of the EPUB.
Socket and HTTP entry points reject network access. No remote success is simulated.

| Plugin | Baseline workflow | State | Test name | What works and what is missing |
|---|---|---|---|---|
| pencil.koplugin | Open an EPUB, enable Pencil, draw a stroke, erase it and reopen the book. | works with limits | `pencil_native_ink_draws_erases_bookmarks_and_reopens` | The pinned enable, tool and undo callbacks use native ink commands. The app test checks pen coordinates, pressure, bookmarks, suspend cancellation, extension unload/reload, saved strokes and erasing after restart. Rust retains input and storage ownership. Upstream framebuffer patches, image previews, highlighter gestures and physical Kobo behavior are not covered. |
| pagenotes.koplugin | Open a PDF, use Add note or sticker, type a text note, place it, finish editing and paint the page. | works with limits | `pagenotes_adds_and_paints_a_pdf_note` | Test adds text through the real dialog, checks stored host settings and changed page pixels, then turns a real PDF page and checks that the note disappears. The app host now persists arbitrary per-book settings; a full app restart test for pagenotes is still missing. EPUB word anchors, sticker placement, dragging, rotation and colour output remain untested. |
| footcream.koplugin | Open an EPUB, scan its measurements and show one metric conversion at the matching text. | works with limits | `footcream_scans_epub_and_paints_a_tappable_conversion`; `footcream_native_worker_scan_and_tap` | CREngine supplies document-wide text search, xpointers and screen boxes. Tests check a match beyond page one, painted underlines, conversion taps, cached matches after reopen, and app suspend/reload. Caches use Kaia data. EPUB rewriting, report uploads and update downloads are disabled. The source EPUB and existing sidecar fixture stay unchanged. |
| anki.koplugin | Open an EPUB, look up a word, choose Add to Anki and save one card to the offline queue. | works with limits | `anki_native_selection_saves_one_offline_card_across_restart`; `anki_dictionary_button_opens_its_profile_prompt` | A native selection and local dictionary supply the word, definition and surrounding page text. The pinned dictionary button writes one offline card under Kaia settings. The app test checks its fields and unchanged queue after restart, with zero socket or HTTP attempts. The AnkiConnect availability probe is disabled. Online sync, translation and audio are outside this workflow. |
| highlightsync.koplugin | Merge local annotation fixtures into an open EPUB and reopen it. | works with limits | `highlightsync_local_merge_keeps_host_ids_and_reopens` | The pinned merge algorithm writes through the host annotation adapter. Position matches retain host IDs despite incoming IDs or edited text. The test preserves incoming edits, then a newer local edit, rejects an invalid batch without a partial write, and reopens the saved annotations. Cache files stay under Kaia data; existing KOReader sidecars stay unchanged. Remote sync reports that only local merges are supported. There is no local-file picker or cloud workflow claim. |
| readingstreak.koplugin | Open an EPUB, turn a page, view the streak and calendar, then reopen the plugin. | works with limits | `readingstreak_tracks_real_page_turns`; `community_readingstreak_records_app_page_turns_and_reopens` | Checks one page increment, paints the streak and calendar widgets, and checks the saved streak after reopening. The app test turns the native PDF page and reopens the book, preserving the page total below the daily goal. Time thresholds, day rollover, history import, and persistence of page totals after completing a daily goal remain untested. |
| bookends.koplugin | Open a PDF, enable Bookends through its menu, paint the default overlay, flush settings and reopen. | works with limits | `bookends_paints_an_overlay` | Checks changed pixels and the plugin's own paint error counter before and after reopening. Tests the default preset on a grayscale frame. Advanced tokens, colour output, font packs, skimming, update downloads and preset sharing remain untested. The app now paints registered overlays. A Bookends-specific app workflow test is still missing. |
| readeck.koplugin | Open a book, configure Readeck and its download folder, then sync and open one article. | blocked | `readeck_opens_settings_and_requires_a_server` | Settings open. The sync action runs its scheduled callback and reports the missing server and download folder, with sync_in_progress cleared. Article download, authentication and export require server access, excluded by the no-network product rule. Successful transfer is not claimed. |
| foldermemory.koplugin | In a library containing an EPUB, save a folder's sort/display settings, leave it and return. | blocked | `foldermemory_requires_library_display_hooks` | Library-mode load stops at FileManager.file_chooser. Reader-mode investigation also reached FileManager.onRefresh. The plugin patches Lua FileChooser and CoverBrowser. Needs app library adapters for sort, filters, display mode, grid size and folder entry/refresh. Lua class patches cannot change the Rust library view. |

`safe_mode_excludes_the_pinned_community_list` checks that safe mode skips all
nine extra-path plugins. The workflows run plugins separately. Coexistence of
all nine plugins in one session is not claimed.

## Fixture provenance

Each folder in `rust/kaia-lua/tests/community` has the original LICENSE and a
PINNED.txt with its source URL, full commit and retrieval date. `manifest.json`
records each source path, SHA-256 hash and omitted path. `verify.py` checks the
file list and hashes offline. The original files remain byte-for-byte unchanged.

Runtime Lua, small icons and stickers, translations and source notices are kept.
Repository automation, upstream tests, screenshots and non-runtime build files
are omitted. No large binary asset or external executable is included. The tested
workflows need no omitted runtime asset. Pencil's separate root `input.lua` is not
installed or vendored as a frontend replacement. See its pinned README for that
prerequisite. Optional Bookends font packs and online services are not supplied.

## Layer changes and app requirements

The reader proxy now exposes the host's live current page as `view.state.page`,
reports its single visible page, and supplies the visible-reader flag used by
UIManager:setDirty. Optional typeset and vocabulary modules can be absent. The
view reports no page-flipping widget. These are layer changes, not plugin patches.
The test window closer now closes invisible windows and has a bounded loop.

The app now implements ReaderHost and LibraryHost, delivers reader lifecycle and
page events, exposes plugin menus and paints registered overlays. Arbitrary
per-book values persist under data/plugin-books. The app tests listed above
exercise readingstreak and Anki through native navigation and dictionary panels.
`safe_mode_skips_user_community_plugin` checks the app startup safe-mode path
with readingstreak. User plugins default to off.

Pencil, Footcream, Anki and HighlightSync now pass their scoped desktop workflows.
Pencil uses the existing native ink store. Footcream uses CREngine queries on the
app document worker and an overlay above the native page. Anki receives the
selection context before the native selection clears. HighlightSync commits its
local merge through BookSession and refreshes the host annotation list.

The isolated layer still rejects Pencil without a native ink host and Anki
without a dictionary host. Those checks describe missing hosts in that test
configuration, not blockers in the Rust app. Library display hooks remain open.
Physical Kobo pen behavior and coexistence of all nine plugins remain untested.

## Inventory maintenance

`curation.json` stores the original pins, licenses, reasons, baselines and test
outcomes. The workflow table above supersedes its four blocked outcomes for
B-19 through B-22.
The plugin inventory generator reads community names, URLs and pins, but hardcodes
the old claim that nothing is downloaded. After running it, the community intro
and the phrase "Version to pin" were corrected by hand in `plugins.md`. Its bundled
table remains generated. Regeneration first failed because the existing local
kaiaexample and kaiastickers plugins had no curation entries. Their entries now
describe the source and proposed workflows without claiming Lua compatibility.
Updating the generator itself is outside this change's
allowed paths. Preserve those small wording corrections on future regeneration.

`LICENSES.md` has a manually maintained community supplement because its generator
does not inventory these fixtures. Preserve that supplement on regeneration.

## Original pinning validation, 2026-10-13

From the repository root:

```sh
python3 rust/kaia-lua/tests/community/verify.py
python3 -I tools/kaia/inventory/plugin_inventory.py --root . --out docs/kaia/inventory
source tools/kaia/env.sh
cd rust
cargo fmt -p kaia-lua -- --check
cargo clippy -p kaia-lua --all-targets -- -D warnings
cargo test -p kaia-lua
```

The final format check produced no output and exited 0. Clippy exited 0. The
full test run passed 98 integration tests and one documentation test, including
ten tests in community.rs. The hash checker verified 402 upstream files. An
additional `gh api` check compared every vendored file with its Git blob ID at
the pinned commit. All 402 matched.

## B-19 through B-22 validation

Run from this worktree's `rust/` directory. Keep build output outside the shared
checkout:

```sh
source ../tools/kaia/env.sh
export CARGO_TARGET_DIR=$HOME/kaia-wt-plugins-target
cargo test -p kaia-lua --test community --no-fail-fast
cargo test -p kaia-app --test plugins --no-fail-fast
cargo test --workspace --no-fail-fast
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy -p kaia-app --no-default-features --features kobo --all-targets -- -D warnings
cargo fmt --all -- --check
```

The community layer passed all 10 tests, and the app plugin suite passed all 15
tests. `cargo test --workspace --no-fail-fast` exited 0 with 1,926 tests
passed, no failures and 3 ignored tests. The ignored tests cover local real
dictionaries, a release-mode dictionary benchmark and graphics timings.
Workspace Clippy, Kobo Clippy and the format check passed.

The offline audit verified all 183 upstream files in the four plugin fixture
trees against their pinned hashes. The full offline fixture checker
currently fails because this checkout lacks Readeck's manifest-listed
`readeck/i18n/zh_cn.lua`. Readeck is outside B-19 through B-22; no fixture was
fetched or substituted.
