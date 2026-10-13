# Bundled plugin desktop status

L5 part 1, 2026-10-13. These states describe desktop test results.
All 37 bundled entries in `plugins.json` have a load-outcome
assertion in `bundled_inventory_load_outcomes`. A successful load alone does
not count as a working workflow.

The workflow tests are in `rust/kaia-lua/tests/bundled.rs`. They use the original
plugin code and KOReader widgets on the Kaia window stack. Menu actions and
editable dialogs receive taps and key events. Test hosts use `BookSession` for
the repository's EPUB and PDF. Fixtures supply initial annotations, paths and
lookup events where the Rust app normally supplies them. No test connects to a
server. Network tests reject socket and HTTP calls.

Summary: 2 work, 20 work with limits, 11 are blocked, and 4 are not applicable.
The limits below include untested steps and hardware exclusions. L5 remains
incomplete.

| Plugin | State | Test name | What works and what is missing |
|---|---|---|---|
| SSH | blocked | `bundled_inventory_load_outcomes` | Its gate requires the patched dropbear binary, absent from this emulator tree. Server start and host connections are excluded. No settings workflow claim. |
| archiveviewer | works with limits | `archiveviewer_lists_views_and_extracts_an_epub_member` | Lists a real EPUB archive, views its text member, and extracts it into a scratch folder. Image preview and file browser Open with integration remain untested. |
| autodim | blocked | `bundled_inventory_load_outcomes` | The desktop shim has no frontlight capability or `Device.powerd`. Needs intensity readings and writes from the device host. Hardware dimming is excluded. |
| autostandby | not applicable | `bundled_inventory_load_outcomes` | Upstream enables it only on PocketBook. |
| autosuspend | blocked | `bundled_inventory_load_outcomes` | The desktop shim cannot suspend. Needs host suspend, power-off, wakeup and standby-hold integration. Physical suspend and power-off are excluded. |
| autoturn | works with limits | `autoturn_uses_input_hook_and_advances_the_reader` | Sets a timeout through the picker, runs the scheduler and changes the PDF page. Input hooks run. Fractional EPUB scrolling and restart persistence remain untested. |
| autowarmth | blocked | `bundled_inventory_load_outcomes` | Loads without warmth controls. Needs frontlight warmth and night-mode events connected to the app and device. Schedule effects are not claimed. |
| batterystat | blocked | `bundled_inventory_load_outcomes` | Fails on missing `getPowerDevice()` readings. Needs battery capacity and charging state. Hardware samples are excluded. |
| bookshortcuts | works with limits | `bookshortcuts_opens_a_saved_book_and_keeps_settings` | Opens a seeded book shortcut through `LibraryHost` and writes the settings file. Folder creation, deletion and binding that shortcut through the gesture picker remain untested. |
| calibre | works with limits | `offline_network_settings_dialogs` | Opens wireless settings and the password dialog. Discovery, connection and transfer are excluded. |
| cloudstorage | works with limits | `opds_and_cloudstorage_open_local_lists_without_network` | Opens the server list and add-server choices. Authentication and remote browsing are excluded. |
| coverbrowser | blocked | `bundled_inventory_load_outcomes` | Applicable on Kobo. Fails on `FileManager.addFileDialogButtons`; it also patches FileChooser, History and Collections rendering. Decision: do not replace the Rust browser with those Lua classes. The app needs display-mode and cover-grid adapters. No mosaic workflow claim. |
| coverimage | not applicable | `bundled_inventory_load_outcomes` | Upstream's platform gate excludes Kobo. |
| docsettingtweak | works with limits | `docsettingtweak_applies_directory_defaults` | Types and saves a folder rule in the directory-defaults editor, then applies it through `doc_settings.data` and flush to an EPUB font size. A shim supplies the missing `loadDefaults` method called by the bundled editor. Changing an existing rule and restarting remain untested. The app must deliver DocSettingsLoad before first layout. |
| exporter | works with limits | `exporter_writes_four_formats_from_real_books`; `annotation_writes_use_host_ids_and_survive_reopening` | Exports highlights and notes from a real EPUB and PDF to Markdown, HTML, JSON and text. Annotation add, note update, delete and reopen use host ids. Color-filter persistence and repeat-export equality remain untested. Remote targets are excluded. |
| externalkeyboard | blocked | `bundled_inventory_load_outcomes` | The required OTG role file is absent on the desktop. USB connection, key mapping and role changes need the Kobo input/device path. Hardware steps are excluded. |
| gestures | works with limits | `gestures_binds_a_plugin_action_and_fires_it` | Changes a corner tap binding in the menu, restarts, and fires the bundled readtimer action through `ActionRegistry`. Only registered plugin actions and the supplied font actions are supported. Default reader actions, override ordering, and multiswipe recording need further work. |
| hello | not applicable | `bundled_inventory_load_outcomes` | Upstream disables this developer template on all platforms. The earlier L4 forced-load test remains. |
| hotkeys | not applicable | `bundled_inventory_load_outcomes` | Libra Colour has neither a screen keyboard capability nor a physical keyboard capability in this gate. |
| httpinspector | works with limits | `offline_network_settings_dialogs` | Opens the port dialog. Listening on a socket, browser requests and auto-start are excluded. |
| japanese | works with limits | `japanese_deinflects_and_opens_scan_settings` | Runs the bundled deinflector and changes scan length through its language submenu. No StarDict fixture is installed; lookup and selection expansion need the dictionary host. |
| keepalive | blocked | `bundled_inventory_load_outcomes` | The desktop device identity does not enter the Kobo branch. The app must connect the shared pause-auto-suspend flag to its idle policy. Physical idle/suspend verification is excluded. |
| kosync | works with limits | `offline_network_settings_dialogs` | Opens the custom-server input and reads the current page on ReaderReady. Registration, login and progress transfer are excluded. |
| movetoarchive | works with limits | `movetoarchive_copies_and_moves_scratch_books` | Copies and moves temporary PDFs through `LibraryHost`, opens the resulting path and saves the archive setting. The archive directory is seeded. Directory selection, restart restoration and Rust library index updates remain untested. |
| newsdownloader | works with limits | `offline_network_settings_dialogs` | Opens settings and changes the image-download option. Feed retrieval and downloaded article creation are excluded. Feed-list editing is not verified. |
| opds | works with limits | `opds_and_cloudstorage_open_local_lists_without_network` | Opens the local catalog list and add-catalog dialog. Fetch, search, download and catalog sync are excluded. Saving a new catalog is not verified. |
| perceptionexpander | works | `perceptionexpander_paints_over_a_real_page_and_persists` | Enables the overlay, types margin, thickness and intensity settings, paints over a real PDF and turns a page. Sends suspend/resume, flushes, restarts, checks settings and painted bytes, then turns another page. The app must call the overlay paint entry point. |
| profiles | works with limits | `profiles_restores_real_epub_font_size` | Captures a profile through the name dialog, changes the real EPUB font size, executes the profile and restores that size. The profile survives reopening. The full configurable setting list and auto-execute rules remain untested. |
| qrclipboard | works with limits | `qrclipboard_renders_clipboard_text` | Reads the shim clipboard and paints a QR widget. Clipboard persistence, scanning the code and the highlight-dialog entry are not verified. |
| readtimer | works with limits | `readtimer_alarm_runs_on_the_kaia_scheduler` | Sets a one-minute interval through the picker, displays expiry after simulated suspend/resume, repeats it and dismisses the second alarm. Registers and paints additional footer content. Reopening preserves the footer preference and the stopped state. Restarting while an alarm is active remains untested. |
| statistics | works with limits | `statistics_records_pages_and_opens_views` | Records timed page events, checks stored page/time totals, opens book, progress, calendar and timeline views, and verifies totals after reopening. Wall-clock time is simulated. Color preference changes remain untested. |
| systemstat | blocked | `bundled_inventory_load_outcomes` | Fails on missing power readings. Needs the device host's battery and charging state; Kobo storage and `/proc` rows require device verification. |
| terminal | blocked | `terminal_settings_and_session_prerequisites` | Opens font settings. A separate session attempt left a forked child stuck before exec in the multithreaded Rust test process; the test now stops before fork. Needs app-owned process and PTY I/O. Command execution is not claimed. |
| texteditor | works | `texteditor_creates_saves_and_reopens_a_file` | Creates a scratch file through the filename dialog, types and saves text, changes font size and monospace, and reopens it from history. Sends suspend/resume, restarts and confirms the contents and font preferences persist. |
| timesync | blocked | `bundled_inventory_load_outcomes` | Requires root and busybox ntpd or ntpdate. Time-server access and system clock changes are excluded. |
| vocabbuilder | works with limits | `vocabulary_records_words_and_reviews_a_card` | Stores three lookup events in SQLite, opens the word list and answers a due card. Dictionary lookup is supplied as an event. Deleting words, dictionary popup buttons and progress after restart remain untested. |
| wallabag | works with limits | `offline_network_settings_dialogs` | Opens server configuration. Login, article download, upload and sync are excluded. |

## Changes needed outside kaia-lua

No other crate was edited for this work.

- `kaia-app` must implement the extended `ReaderHost` annotation methods and
  settings flush, plus `LibraryHost` copy, move, folder navigation and open-book
  methods. The test hosts implement them; the production app is not connected.
- `kaia-app` must call `LuaUi::paint_reader_overlays` after painting the page and
  before dialogs. It must route input and reader lifecycle events to `LuaUi`,
  deliver DocSettingsLoad before first layout, and expose its supported actions
  to the plugin dispatcher. The gesture shim does not implement every reader action.
- The device/app boundary needs battery, charging, frontlight intensity, warmth,
  suspend, power-off and wakeup services, plus standby holds and the shared
  pause-auto-suspend flag. The layer must not advertise those capabilities before
  the host can execute them.
- The dictionary/app boundary needs lookup, selection context and dictionary popup
  button registration for Japanese and vocabulary builder's complete baselines.
- Coverbrowser needs an app library adapter for cover indexing, display modes,
  grid sizing, History and Collections. Patching Lua FileChooser methods cannot
  change the Rust library view.
- Terminal needs process creation and nonblocking PTY reads/writes owned by the
  Rust app. The Lua plugin's raw fork is unsuitable inside this test runner.
- Kobo packaging/device tests must provide dropbear, the OTG role path and the
  time-sync prerequisites before those gated workflows can be assessed.

## Validation

After sourcing `tools/kaia/env.sh`, the final runs passed:

- `cd rust && cargo fmt -p kaia-lua -- --check`, exit 0 with no output.
- `cargo clippy -p kaia-lua --all-targets -- -D warnings`, exit 0.
- `cargo test -p kaia-lua`, 88 integration tests and 1 documentation test passed.
  This includes 20 tests in `tests/bundled.rs`.

An earlier Clippy run failed on `kaia-book`'s `large_enum_variant` lint. The
final run no longer reports it. No file in that crate was changed by this work.
No golden PNG was added.
