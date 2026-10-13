# Bundled plugin desktop status

L5 part 1, 2026-10-13. These states describe desktop test results.
All 37 upstream bundled entries have a load-outcome
assertion in `bundled_inventory_load_outcomes`. A successful load alone does
not count as a working workflow. The regenerated inventory also lists the local
`kaiaexample` and `kaiastickers` plugins. This table does not assess those two.

The layer workflow tests are in `rust/kaia-lua/tests/bundled.rs`. They use the original
plugin code and KOReader widgets on the Kaia window stack. Menu actions and
editable dialogs receive taps and key events. Test hosts use `BookSession` for
the repository's EPUB and PDF. Fixtures supply initial annotations, paths and
lookup events where the Rust app normally supplies them. No test connects to a
server. Network tests reject socket and HTTP calls.

Summary: 2 work, 22 work with limits, 9 are blocked, and 4 are not applicable.
The limits below include untested steps and hardware exclusions. L5 remains
incomplete.

| Plugin | State | Test name | What works and what is missing |
|---|---|---|---|
| SSH | blocked | `bundled_inventory_load_outcomes` | Its gate requires the patched dropbear binary, absent from this emulator tree. Server start and host connections are excluded. No settings workflow claim. |
| archiveviewer | works with limits | `archiveviewer_lists_views_and_extracts_an_epub_member` | Lists a real EPUB archive, views its text member, and extracts it into a scratch folder. Image preview and file browser Open with integration remain untested. |
| autodim | blocked | `bundled_inventory_load_outcomes` | The app now supplies frontlight readings and writes. `plugin_device_requests_use_platform_and_idle_flags` checks those services with a fixture. The autodim scheduling and ramp workflow still needs a test. Hardware dimming is excluded. |
| autostandby | not applicable | `bundled_inventory_load_outcomes` | Upstream enables it only on PocketBook. |
| autosuspend | blocked | `bundled_inventory_load_outcomes` | The app supplies suspend requests and standby holds. Power-off and wakeup services remain absent. The plugin timer workflow and physical suspend are untested. |
| autoturn | works with limits | `autoturn_uses_input_hook_and_advances_the_reader` | Sets a timeout through the picker, runs the scheduler and changes the PDF page. Input hooks run. Fractional EPUB scrolling and restart persistence remain untested. |
| autowarmth | blocked | `bundled_inventory_load_outcomes` | The app supplies warmth readings and writes. Night-mode events and the scheduled warmth workflow remain untested. |
| batterystat | works with limits | `plugin_device_requests_use_platform_and_idle_flags` | The app opens the battery statistics dialog and delivers suspend/resume with desktop battery and charging readings. Hardware samples and discharge history are untested. |
| bookshortcuts | works with limits | `bookshortcuts_opens_a_saved_book_and_keeps_settings` | Opens a seeded book shortcut through `LibraryHost` and writes the settings file. Folder creation, deletion and binding that shortcut through the gesture picker remain untested. |
| calibre | works with limits | `offline_network_settings_dialogs` | Opens wireless settings and the password dialog. Discovery, connection and transfer are excluded. |
| cloudstorage | works with limits | `opds_and_cloudstorage_open_local_lists_without_network` | Opens the server list and add-server choices. Authentication and remote browsing are excluded. |
| coverbrowser | blocked | `bundled_inventory_load_outcomes` | Applicable on Kobo. Fails on `FileManager.addFileDialogButtons`; it also patches FileChooser, History and Collections rendering. Decision: do not replace the Rust browser with those Lua classes. The app needs display-mode and cover-grid adapters. No mosaic workflow claim. |
| coverimage | not applicable | `bundled_inventory_load_outcomes` | Upstream's platform gate excludes Kobo. |
| docsettingtweak | works with limits | `docsettingtweak_applies_directory_defaults` | Types and saves a folder rule in the directory-defaults editor, then applies it through `doc_settings.data` and flush to an EPUB font size. A shim supplies the missing `loadDefaults` method called by the bundled editor. Changing an existing rule and restarting remain untested. The app must deliver DocSettingsLoad before first layout. |
| exporter | works with limits | `exporter_writes_four_formats_from_real_books`; `annotation_writes_use_host_ids_and_survive_reopening`; `reader_plugin_gets_page_update_from_app_page_turn` | Exports highlights and notes from a real EPUB and PDF to Markdown, HTML, JSON and text. Annotation add, note update, delete and reopen use host ids. Color-filter persistence and repeat-export equality remain untested. Remote targets are excluded. |
| externalkeyboard | blocked | `bundled_inventory_load_outcomes` | The required OTG role file is absent on the desktop. USB connection, key mapping and role changes need the Kobo input/device path. Hardware steps are excluded. |
| gestures | works with limits | `gestures_binds_a_plugin_action_and_fires_it` | Changes a corner tap binding in the menu, restarts, and fires the bundled readtimer action through `ActionRegistry`. Only registered plugin actions and the supplied font actions are supported. Default reader actions, override ordering, and multiswipe recording need further work. |
| hello | not applicable | `bundled_inventory_load_outcomes` | Upstream disables this developer template on all platforms. The app test `settings_plugin_menu_opens_a_foreign_dialog_and_tap_closes_it` forces it on, opens its menu from Extensions and dismisses the real dialog with a tap. |
| hotkeys | not applicable | `bundled_inventory_load_outcomes` | Libra Colour has neither a screen keyboard capability nor a physical keyboard capability in this gate. |
| httpinspector | works with limits | `offline_network_settings_dialogs` | Opens the port dialog. Listening on a socket, browser requests and auto-start are excluded. |
| japanese | works with limits | `japanese_deinflects_and_opens_scan_settings` | Runs the bundled deinflector and changes scan length through its language submenu. No StarDict fixture is installed; lookup and selection expansion need the dictionary host. |
| keepalive | blocked | `bundled_inventory_load_outcomes` | The desktop device identity does not enter the Kobo branch. The app now honors the shared pause-auto-suspend flag and standby holds, tested by `plugin_device_requests_use_platform_and_idle_flags`. The bundled Kobo-only workflow remains untested. Physical idle/suspend verification is excluded. |
| kosync | works with limits | `offline_network_settings_dialogs` | Opens the custom-server input and reads the current page on ReaderReady. Registration, login and progress transfer are excluded. |
| movetoarchive | works with limits | `movetoarchive_copies_and_moves_scratch_books` | Copies and moves temporary PDFs through `LibraryHost`, opens the resulting path and saves the archive setting. The archive directory is seeded. Directory selection, restart restoration and Rust library index updates remain untested. |
| newsdownloader | works with limits | `offline_network_settings_dialogs` | Opens settings and changes the image-download option. Feed retrieval and downloaded article creation are excluded. Feed-list editing is not verified. |
| opds | works with limits | `opds_and_cloudstorage_open_local_lists_without_network` | Opens the local catalog list and add-catalog dialog. Fetch, search, download and catalog sync are excluded. Saving a new catalog is not verified. |
| perceptionexpander | works | `perceptionexpander_paints_over_a_real_page_and_persists` | Enables the overlay, types margin, thickness and intensity settings, paints over a real PDF and turns a page. Sends suspend/resume, flushes, restarts, checks settings and painted bytes, then turns another page. The app test `reader_paints_perception_overlay_and_plugin_footer` checks the guides and footer over a native PDF page, including removal when plugins are disabled. |
| profiles | works with limits | `profiles_restores_real_epub_font_size` | Captures a profile through the name dialog, changes the real EPUB font size, executes the profile and restores that size. The profile survives reopening. The full configurable setting list and auto-execute rules remain untested. |
| qrclipboard | works with limits | `qrclipboard_renders_clipboard_text` | Reads the shim clipboard and paints a QR widget. Clipboard persistence, scanning the code and the highlight-dialog entry are not verified. |
| readtimer | works with limits | `readtimer_alarm_runs_on_the_kaia_scheduler` | Sets a one-minute interval through the picker, displays expiry after simulated suspend/resume, repeats it and dismisses the second alarm. Registers and paints additional footer content. Reopening preserves the footer preference and the stopped state. Restarting while an alarm is active remains untested. |
| statistics | works with limits | `statistics_records_pages_and_opens_views`; `reader_plugin_gets_page_update_from_app_page_turn` | Records timed page events, checks stored page/time totals, opens book, progress, calendar and timeline views, and verifies totals after reopening. The app test turns the native reader page and checks the plugin's current page, then opens its statistics menu. Layer tests simulate wall-clock time. Color preference changes remain untested. |
| systemstat | works with limits | `plugin_device_requests_use_platform_and_idle_flags` | The app opens system statistics and delivers suspend/resume with desktop power readings. Kobo storage and `/proc` rows require device verification. |
| terminal | blocked | `terminal_settings_and_session_prerequisites` | Opens font settings. A separate session attempt left a forked child stuck before exec in the multithreaded Rust test process; the test now stops before fork. Needs app-owned process and PTY I/O. Command execution is not claimed. |
| texteditor | works | `texteditor_creates_saves_and_reopens_a_file` | Creates a scratch file through the filename dialog, types and saves text, changes font size and monospace, and reopens it from history. Sends suspend/resume, restarts and confirms the contents and font preferences persist. |
| timesync | blocked | `bundled_inventory_load_outcomes` | Requires root and busybox ntpd or ntpdate. Time-server access and system clock changes are excluded. |
| vocabbuilder | works with limits | `vocabulary_records_words_and_reviews_a_card`; `dictionary_lookup_reaches_vocabulary_builder` | Stores three lookup events in SQLite, opens the word list and answers a due card. The app test looks up a word in the native dictionary panel and checks the plugin SQLite row. It also opens the dictionary plugin actions window. Deleting words, review buttons and progress after restart remain untested. |
| wallabag | works with limits | `offline_network_settings_dialogs` | Opens server configuration. Login, article download, upload and sync are excluded. |

## App integration and remaining limits

`rust/kaia-app/tests/plugins.rs` tests the app hosts. The app owns one lazy
LuaUi, loads bundled and user plugins, and shares its window stack and scheduler
with Lua dialogs. Extensions stores each plugin switch in settings.json.
The reader's More panel opens plugin menus. Load and dialog errors disable the
owner for the session. Both safe-mode controls skip user plugins.

ReaderHost uses the reader's BookSession for navigation, annotations and saved
layout. Arbitrary plugin values persist in data/plugin-books. A separate document
handle supplies text and xpointer queries with the same engine and layout.
LibraryHost reads the index and implements open, copy, move and folder navigation.
The app paints view modules and footer text after the page and before dialogs.

Remaining work:

- DocSettingsLoad currently arrives after BookSession opens. Directory defaults
  must run before the first layout to complete docsettingtweak's app workflow.
- Text selection methods cover the active selection. Full context extraction,
  dictionary review buttons and Japanese selection expansion need more work.
- Coverbrowser still needs adapters for the native library's display modes,
  cover grid, history and collections. Lua FileChooser patches cannot change it.
- Terminal still needs app-owned process creation and nonblocking PTY I/O.
- Power-off, wakeup, USB keyboard and physical Kobo plugin workflows are untested.
  Both firmware packages contain the pinned target LuaJIT, frontend, plugins
  and compatibility shims.

The app's runtime label "Works" means the plugin loaded without a caught error
in the current context. This table records the narrower tested workflows.

## Validation

App tests cover menus, native page turns, dictionary hooks, device requests,
safe mode, failure isolation, overlays and the default bundled set. The three
new app golden images show the hello menu, its dialog and the reader overlay.
The existing layer tests still cover their separate plugin workflows.

The app and layer use `cargo fmt`, both Lua and no-Lua Clippy builds, their full
Cargo test suites, and both Kobo build/package checks. Hardware behavior is not
part of these desktop checks.

Verification on 2026-10-13 passed the app and layer format check, both requested
Clippy modes, 160 app tests, 98 Lua integration tests and one Lua documentation
test. The final app plugin rerun passed all 12 tests. Both ARM release builds
passed their glibc checks, and each package passed 24 content checks. All three
new golden images were inspected.
