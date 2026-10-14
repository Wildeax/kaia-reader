# Progress sync spec

Status: version 1. Describes the code as of 2026-10-14. Sync has not been tested against a real sync server.

This spec covers reading progress sync in the Rust app. It covers the `kaia-sync` crate, the app glue in `rust/kaia-app/src/sync.rs` and `rust/kaia-app/src/sync_queue.rs`, and the reader hooks in `rust/kaia-app/src/surfaces/reader/mod.rs`.

The protocol follows KOReader's kosync plugin. `plugins/kosync.koplugin/api.json` and `KOSyncClient.lua` define the requests. `main.lua` defines the conflict rules, and `rust/kaia-sync/src/policy.rs` ports them with the line numbers in its comments.

## Consent

Sync is off until the user turns it on. Nothing sends a request unless the user takes one of the actions below.

- Default: `sync.enabled` is false (`rust/kaia-app/src/surfaces/settings/keys.rs`, the settings table).
- Switch: Settings, Network, "Sync reading progress" (`rust/kaia-app/src/surfaces/settings/sections.rs`, function `network`). The switch saves at once and shows no confirmation. The section text says that sync sends progress on close and sleep. With automatic sync on, it also says that a book reads progress from other devices when it opens.
- Automatic sync: Settings, Network, "Automatically keep documents in sync" (`sync.auto`, `rust/kaia-app/src/surfaces/settings/keys.rs`). It is off by default, as in KOReader. It controls pulls on open and on resume, and pushes on save, suspend and close (`ProgressSync::push` in `rust/kaia-app/src/sync.rs`), as KOReader does.
- Test connection: the "Test connection" button sends one `GET /healthcheck`. It needs sync on. With sync off, the toast reads "Turn on progress sync first." (`rust/kaia-app/src/sync.rs`, `ProgressSync::health`). Opening the section or editing its fields sends nothing.
- Automatic requests run only while sync is on. Each one follows a reader event:
  - Opening a book pulls the remote position (`reader/mod.rs`, `on_show`), when `sync.auto` is on.
  - Resume pulls again (`reader/mod.rs`, the `Resume` branch of `on_event`), when `sync.auto` is on.
  - Ten page changes start a push that waits 10 s (`rust/kaia-app/src/surfaces/reader/persistence.rs`, `periodic_sync`).
  - Save, suspend and close push the position (`reader/mod.rs`, the `SaveState` and `Suspend` branch, and `close`).
- Each worker job checks the switch and a refused login when it starts (the worker closure in `ProgressSync::submit`). Each queued push checks the switch again before it sends (`push_queue`).
- No timer fetches progress while the reader is closed. The queue drains only when a pending push comes due, and pending pushes come from reader events.
- Pulls on open follow `sync.auto`, with a default of off as in KOReader (`plugins/kosync.koplugin/main.lua`, line 61). The app reads the setting into the policy (`reader/mod.rs`, `receive_sync`). It also gates the pull itself (`ProgressSync::automatic_pull_allowed`, called from `ProgressSync::pull`). The crate default is off too (`rust/kaia-sync/src/policy.rs`, `Policy::default`).
- Airplane mode holds sync. While `network.airplane_mode` is on (`keys::airplane_mode`), `ProgressSync::pull` and `ProgressSync::tick` send nothing, and no toast shows for an attempt. Due pushes go to `sync-queue.json` and wait there. The queue drains when a push comes due with airplane mode off. Test connection shows "Airplane mode is on. Turn it off to use Wi-Fi." Airplane mode turns Wi-Fi off and refuses to turn it on again (`rust/kaia-app/src/app.rs`, the `settings:quick:airplane` and `settings:quick:wifi` handling). Sync never turns Wi-Fi on.
- A login the server refuses with 401 holds the requests that use that login. The first refusal shows one message. The hold ends when the user types a password (`ProgressSync::forget_refusal`), or when the account, the address or the key changes (`is_refused`). The hold lives in memory, so a restart sends one request again.
- No default server exists. The app does not use `DEFAULT_SERVER` (`rust/kaia-sync/src/client.rs`), which only the tests use. With the switch on, `sync.auto` on and no server or username, every book open starts a pull that fails with an error toast (see Failures).

## Credentials

What the app stores:

| Key | Where | Form | Notes |
|---|---|---|---|
| `sync.server` | `settings.json` | Text | The address as typed. |
| `sync.username` | `settings.json` | Text | Trimmed when read. |
| `sync.userkey` | `settings.json` | MD5 hex, 32 characters | The only password-derived value. |
| `sync.device_id` | `settings.json` | MD5 hex | Made at first start from the process id, the time and the data folder (`rust/kaia-app/src/services.rs`). It identifies this device, and it is not a secret. |
| `sync.accept_invalid_certs` | `settings.json` | Boolean | Default false. |
| `sync.password` | Not stored | | An old plain password is removed at startup. |

Rules:

- The typed password is never stored. The password field calls `store_password` (`rust/kaia-app/src/sync.rs`). That function saves `userkey_for(password)` as `sync.userkey` and deletes `sync.password`. The field starts empty (`sections.rs`, the `SYNC_PASSWORD` entry).
- A blank password removes `sync.userkey`. The next request then fails with "invalid password".
- An older settings file with a plain `sync.password` is migrated at startup (`migrate_password_file`, called from `rust/kaia-app/src/services.rs`). The function writes the key, flushes the file, and removes `settings.json.old` when that copy holds the password.
- The user key is a credential. It travels in the `x-auth-key` header on each authenticated request (`SyncClient::exchange`, `rust/kaia-sync/src/client.rs`). It also travels as the `password` field at registration (`SyncClient::register`). KOReader does the same (`main.lua`, `doLogin` and `doRegister`). Anyone who can read `settings.json` can sign in as that user. The file is not encrypted.
- Plain HTTP sends the username and the key without encryption. The app accepts `http://` addresses and shows no warning for them. HTTPS protects them unless the user turns on "Accept self-signed certificates", which skips certificate checks for all sync servers.
- The typed password, the request headers and the response bodies beyond the error message are never stored or logged. `Credentials` prints `<redacted>` in its `Debug` output (`rust/kaia-sync/src/model.rs`). The app writes errors to toasts and to stderr only at shutdown (`rust/kaia-app/src/app.rs`). The sync queue holds progress entries, which carry no credentials.

## Data ownership

Reads:

- The `sync.*` keys in `settings.json`.
- The open book's partial MD5, its position and its percent, from the book session (`kaia-book`).
- `sync-queue.json` in the data folder.

Writes:

- `sync-queue.json` in the data folder, through the app's `SyncQueue` (`rust/kaia-app/src/sync_queue.rs`). The queue takes a lock file, `sync-queue.json.lock`, writes a staged copy, and renames it into place.
- `settings.json`. The `sync.*` keys are saved when the Settings section closes or the app shuts down (`rust/kaia-app/src/surfaces/settings/keys.rs`, the module comment).
- The reading position, when the user accepts a remote position. The reader moves with `core.go_to` (`reader/mod.rs`). The book state then saves the position as it does for any move.

Never touched: KOReader sidecars (`.sdr` folders, docsettings, `history.lua`), `statistics.sqlite3`, `library.sqlite3`, book files and the Kobo database. The sync code has no path to them.

This follows `docs/kaia/specs/STORAGE.md`. The book state belongs to `kaia-book`, and the settings belong to `kaia-app`.

A queue entry holds the document id, the position text, the percent, the device name (see Compatibility), the device id, and a null timestamp. It holds no title, no path and no metadata (`ProgressSync::push` sets `metadata: None`).

## Cancellation and timeouts

Timeouts (`rust/kaia-sync/src/client.rs`, `Timeouts::default` and `agent`):

- Progress calls (pull, push): 2 s per socket phase, 5 s for the whole call.
- Login calls (register, authorize, health): 5 s per socket phase, 10 s for the whole call.
- Response body: 64 KiB at most (`MAX_BODY`). Larger bodies fail with a protocol error.
- Redirects: none. A 3xx reply counts as a server error, so credentials never follow a redirect.

Cancellation:

- A request in flight cannot be cancelled. The only controls are the switch and the timeouts.
- A job checks the switch when it starts and before each queued push. Turning sync off clears the pending pushes (`ProgressSync::tick`). Requests already in flight finish or time out.
- Closing the app detaches the worker without waiting for it (`ProgressSync::close`). A test covers this.
- A pull for the same book within 25 s is skipped (`pull_debounced` in `rust/kaia-sync/src/queue.rs`, and `ProgressSync::pull`). A pending push waits 1 s. A periodic push waits 10 s after the last page change (`PeriodicPush`).

State left behind:

- A pending push that has not been sent is written to `sync-queue.json`. Writes happen on each `tick`, at suspend and close (`persist_pending`), and at shutdown.
- A failed push stays in the queue. The drain stops at the first failure and keeps that entry and the later ones.
- An update that the server refuses with 401 is dropped, as KOReader drops it. The drain stops there, and the later updates stay queued until the login changes (see Consent).
- An entry leaves the queue after the server accepts it. If the app stops after the server accepts an entry and before the entry leaves the queue, the entry is sent again on the next drain.
- The queue keeps one entry per document per UTC day, at most 200 entries, and drops entries older than 28 days (`rust/kaia-app/src/sync_queue.rs`, `push`).
- A pull is not queued. A failed pull shows a toast and nothing else.

## Failures

The app shows each failure as a toast. The text ids below are the English source strings from the gettext calls.

| Failure | Where it comes from | What the user sees |
|---|---|---|
| Sync off when Test connection is tapped | `ProgressSync::health` | "Turn on progress sync first." |
| Server answers the health check | `ProgressSync::health` and `app.rs`, the `Reply::ServerAnswered` branch | "The server answered." |
| Any other sync failure | `app.rs`, the `Reply::Error` branch | "Progress sync failed: %1", with the translated text from `error_text` (`rust/kaia-app/src/sync.rs`) in place of `%1` |
| Failure in Test connection | `ProgressSync::health` and `section.rs`, `sync_test` | The text from `error_text`, translated, with no prefix. |
| Test connection in airplane mode | `ProgressSync::health` | "Airplane mode is on. Turn it off to use Wi-Fi." Nothing is sent. |
| Login refused on push | `push_queue` and `ProgressSync::tick` | "Progress sync failed: %1" with "The server refused the login. Check the account in Network and sync.", once per refusal. |
| Queue file cannot be read or written | `ProgressSync::tick` and `Failure::Queue` in the worker | "Progress sync failed: %1" with "The sync queue could not be read or saved. Try again." The English text in `sync_queue.rs` ("unreadable ... Kept a copy") does not reach the UI. |
| Queue write fails on exit | `app.rs`, `refuse_exit` | Notice "Could not save the reading position for sync. Try again." and a dialog titled "Sync position not saved" with "Keep reading" and "Exit anyway". |
| Newer or older remote position, prompt strategy | `reader/mod.rs`, `receive_sync` | Dialog "Use synced position?" with "Another device is at %1 percent.", "Keep this position" and "Go there". |

The English error texts come from `rust/kaia-sync/src/error.rs`. The main ones are:

- `invalid server address: %1`, for an address without an http or https host.
- `invalid username`, `invalid password` and `invalid username and password`.
- `request timed out`.
- `network error: %1`, for a refused connection or a TLS failure.
- `server returned %1: %2`, with the server's message or "Unknown server error".
- `unauthorized` messages carry the server's message, or "Unauthorized".
- `unexpected response: %1`, for a body that is not the protocol's JSON.

Failure cases that need a note:

- Wrong password on push. The server answers 401. Kaia drops that entry, as KOReader does (`main.lua`, the queue branch of `updateProgress`), and holds the login until the user saves new credentials (see Consent). The later entries stay queued. The message shows once per refusal.
- Accepted but not 200. The server answers 202, and the app treats it as a failure and queues the entry. This matches KOReader. The `accept_202` option in `SyncOptions` is never set by the app.
- No server or username. With sync and `sync.auto` on, each book open shows "Progress sync failed: %1" with "The server address must start with http:// or https://." or "Enter a username in Network and sync.". This comes from the code path and was not run on a device.
- Airplane mode. Nothing is sent and no toast shows (see Consent). Only Test connection shows a message.

## Compatibility

Protocol:

- Requests and responses follow `plugins/kosync.koplugin/api.json`. The `accept` header is `application/vnd.koreader.v1+json`. The login headers are `x-auth-user` and `x-auth-key`.
- The endpoints are `POST /users/create`, `GET /users/auth`, `PUT /syncs/progress`, `GET /syncs/progress/:document` and `GET /healthcheck`. `api.json` lists the statuses KOReader expects: 201 and 402 for registration, 200 and 401 for authorize, 200, 202 and 401 for progress updates, and 200 and 401 for progress reads. A progress read with no stored position answers 200 without a percentage, and the client returns no position for it (`SyncClient::pull`).
- The document id is the partial MD5 that `kaia-id` computes. `kaia-id` tests its sample offsets against KOReader's (`samples_follow_koreader_offsets`). The crate also has the filename MD5 mode, but the app never selects it.
- The position text is a page number for fixed layouts (the page number plus one) and the xpointer for reflowable books (`rust/kaia-app/src/sync.rs`, `position`).
- The percent is rounded down to four decimals before it is sent (`round_percent`, `rust/kaia-sync/src/doc_id.rs`).
- The conflict rules follow `main.lua`. Ports of the rules are in `rust/kaia-sync/src/policy.rs`, with the line numbers in its comments.
- A server timestamp is compared with the local time of the last page turn. Without a timestamp, the percent decides.

Server software:

- The default address in the plugin is KOReader's public server, `https://sync.koreader.rocks:443`. The client targets that API, which other servers can also implement.
- Only Kaia's in-process test server has been used (`rust/kaia-sync/src/test_server.rs`). Sync against a real server is still open. `docs/kaia/STATUS.md` lists it under R6.

Known gaps:

- The device name is "Kaia Reader" until the app calls `ProgressSync::for_device` at startup. The call is not in `rust/kaia-app/src/services.rs` yet. After it, a Kobo sends its model name, "Kobo Libra Colour". KOReader sends the hostname or the device model (`main.lua`, `chosen_device_name`). Kaia has no hostname setting. The reader's local state uses the same name (`reader/mod.rs`, `receive_sync`), so the device matches its own pushes.
- A refused login is not kept across restarts (see Consent). After a restart, the first request sends once more.
- There is no sign-out. The only way to clear the key is to submit a blank password.
- There is no "pull now" action. `kaia_sync::decide_manual` has no caller in the app.
- There is no option to send document metadata. The app never sends it.
- The app does not offer filename matching.
- The comment on `SyncOptions` in `rust/kaia-sync/src/client.rs` says the Settings screen offers the self-signed certificate option per server. The Settings screen has one switch for all sync servers, and the Calibre and OPDS clients do not read it.
- The comment on `SyncOptions` says KOReader does not verify certificates. This spec did not confirm that. The Lua tree sets no `verify` option, so the result depends on the LuaSec defaults.

## Tests

Each contract has at least one row. "No test" means no existing test checks the contract.

| Contract | Test | File |
|---|---|---|
| Sync off by default | No test | |
| Test connection sends nothing until tapped | `sync_test_calls_the_server_only_when_tapped` | `rust/kaia-app/tests/notes_settings.rs` |
| Turning sync off clears pending pushes | No test | |
| Pulls on open follow the remote-position strategy and ask first | `remote_progress_uses_the_selected_strategy_and_waits_for_confirmation` | `rust/kaia-app/tests/reader.rs` |
| Automatic pull follows `sync.auto` in the app | `opening_a_book_sends_no_pull_while_sync_auto_is_off` and `opening_a_book_pulls_once_sync_auto_is_on` | `rust/kaia-app/tests/sync_policy.rs` |
| The automatic pull gate needs sync, `sync.auto`, and no hold | `automatic_pulls_need_auto_sync_and_no_hold` | `rust/kaia-app/src/sync.rs`, `hold_tests` |
| The `sync.auto` switch saves from Settings | `the_network_section_saves_the_sync_auto_switch` | `rust/kaia-app/tests/sync_policy.rs` |
| Automatic pull follows `auto_sync` in the crate | `auto_sync_off_keeps_everything` | `rust/kaia-sync/src/policy.rs` |
| Airplane mode blocks sync | `airplane_mode_sends_no_request_on_open_or_on_test_connection` and `airplane_mode_queues_due_pushes_without_a_request` | `rust/kaia-app/tests/sync_policy.rs`, `rust/kaia-app/src/sync.rs` |
| A refused login drops one update, holds the rest, and shows one message | `a_refused_login_drops_one_update_and_holds_the_rest_until_new_credentials` | `rust/kaia-app/src/sync.rs`, `hold_tests` |
| The device name is the Kobo model name, else "Kaia Reader" | `a_kobo_sends_its_model_name_and_others_send_the_default` | `rust/kaia-app/src/sync.rs`, `hold_tests` |
| No request in the background with the reader closed | No test | |
| Typed password saves only the user key, and blank removes it | `typing_stores_the_user_key_and_never_the_password` | `rust/kaia-app/src/sync.rs`, `password_tests` |
| Typed password saved to settings.json is only the key | `typing_the_password_keeps_only_its_user_key_on_disk` | `rust/kaia-app/tests/sync_password.rs` |
| Old plain password replaced at startup and its old copy removed | `an_older_plain_password_is_replaced_at_startup` and `the_settings_file_loses_the_password_and_its_old_copy` | `rust/kaia-app/tests/sync_password.rs`, `rust/kaia-app/src/sync.rs` |
| The stored key sends the same header as KOReader | `a_login_sends_the_x_auth_key_koreader_sends` and `stored_key_is_the_x_auth_key_koreader_sends` | `rust/kaia-app/tests/sync_password.rs`, `rust/kaia-sync/src/client/tests.rs` |
| The user key is the MD5 of the password, as in KOReader | `userkey_matches_koreader_for_a_known_password` | `rust/kaia-sync/src/client/tests.rs` |
| Blank fields are refused before any request | `credentials_reject_blank_fields` and `the_client_needs_the_key_and_no_password` | `rust/kaia-sync/src/client/tests.rs`, `rust/kaia-app/src/sync.rs` |
| The key restores without the password | `stored_login_restores_without_password` | `rust/kaia-sync/src/client/tests.rs` |
| `Debug` output hides the key | `credentials_debug_hides_the_digest` | `rust/kaia-sync/src/client/tests.rs` |
| The password never reaches a log | No test | |
| The queue keeps one entry per document per day and retries in order | `queue_deduplicates_by_document_and_day_then_retries_in_order` | `rust/kaia-sync/src/queue.rs` |
| The queue expires entries and caps its size | No test | |
| A push made during a drain survives | `queue_keeps_push_made_during_drain` | `rust/kaia-app/src/sync.rs`, `review2_tests` |
| An unreadable queue is kept as a copy | `unreadable_queue_keeps_bytes_and_accepts_retry` | `rust/kaia-app/src/sync.rs`, `queue_recovery_tests` |
| A failed queue write keeps the app open until the next quit | `failed_queue_write_keeps_app_open_until_the_next_quit` | `rust/kaia-app/tests/review3_shutdown.rs` |
| The exit dialog offers exit without the update, which drops it | `second_refusal_offers_exit_anyway_and_exit_drops_the_update` | `rust/kaia-app/tests/review3_shutdown.rs` |
| Keep reading keeps the update for the next quit | `keep_reading_keeps_the_update_for_the_next_quit` | `rust/kaia-app/tests/review3_shutdown.rs` |
| Jumping replaces the pending position | `jump_replaces_pending_sync_position` | `rust/kaia-app/tests/review2.rs` |
| Each request stops after the timeout | `silent_server_times_out` | `rust/kaia-sync/src/client/tests.rs` |
| Timeout values match the plugin | `default_timeouts_match_the_plugin` | `rust/kaia-sync/src/client/tests.rs` |
| Shutdown does not wait for the network worker | `shutdown_does_not_wait_for_network_worker` | `rust/kaia-app/src/sync.rs`, `review2_tests` |
| Cancelling an in-flight request | No test. Not supported. | |
| Pulls debounce for 25 s, and periodic pushes wait 10 s | `debounce_and_page_counter_follow_plugin_delays` | `rust/kaia-sync/src/queue.rs` |
| Unreachable server gives a network error | `unreachable_server_is_a_network_error` | `rust/kaia-sync/src/client/tests.rs` |
| Bad addresses are refused before any request | `new_rejects_bad_addresses_without_connecting` | `rust/kaia-sync/src/client/tests.rs` |
| https is refused in builds without TLS (runs only with `--no-default-features`) | `https_is_refused_without_tls_feature` | `rust/kaia-sync/src/client/tests.rs` |
| Certificate checks happen by default | No test. `sync_options_default_to_verified_certificates_and_no_202` checks the default value only. | `rust/kaia-sync/src/client/tests.rs` |
| An invalid certificate is accepted only when the user asks | No test | |
| Redirects are not followed | No test | |
| Body larger than 64 KiB is refused | No test | |
| 401 gives an unauthorized error | `authorize_accepts_the_right_password_only` and `push_with_wrong_password_is_unauthorized` | `rust/kaia-sync/src/client/tests.rs` |
| 402 on a duplicate registration | `register_creates_user_and_rejects_duplicates` | `rust/kaia-sync/src/client/tests.rs` |
| 202 is a failure unless `accept_202` is set | `status_202_is_retryable_unless_enabled` | `rust/kaia-sync/src/client/tests.rs` |
| Server errors with and without a JSON message | `server_error_with_json_message_is_reported` and `server_error_with_plain_body_uses_default_message` | `rust/kaia-sync/src/client/tests.rs` |
| Malformed body is a protocol error | `pull_with_malformed_body_is_a_protocol_error` | `rust/kaia-sync/src/client/tests.rs` |
| Missing position gives no progress | `pull_without_stored_position_returns_none` and `pull_reply_without_percentage_is_none` | `rust/kaia-sync/src/client/tests.rs` |
| Failure texts in the UI are translated, and none shows the crate's English | `failures_show_catalog_text_not_the_crate_english` | `rust/kaia-app/src/sync.rs`, `hold_tests` |
| Endpoints, methods and fields match `api.json` | `push_sends_only_the_plugin_fields_by_default` and `push_then_pull_round_trips_the_position` | `rust/kaia-sync/src/client/tests.rs` |
| The Accept header is sent | No test | |
| The percent is rounded down | `push_rounds_percentage_down_to_four_decimals` and `round_percent_floors_to_four_decimals` | `rust/kaia-sync/src/client/tests.rs`, `rust/kaia-sync/src/doc_id.rs` |
| Partial MD5 matches KOReader's sampling | `samples_follow_koreader_offsets` | `rust/kaia-id/src/lib.rs` |
| Conflict rules follow `main.lua` | `newer_remote_silent_forward_pulls` through `manual_pull_at_same_position_keeps` | `rust/kaia-sync/src/policy.rs` |
| Same device name and id keeps the position | `same_device_keeps` | `rust/kaia-sync/src/policy.rs` |
| Against a real server | No test | |
