# Calibre wireless spec

Status: version 1. Describes the code as of 2026-10-14. The wireless connection has been tested against Kaia's test peers, not against a real Calibre server.

This spec covers the Calibre features in the Rust app. It has two parts. The wireless device connection sends books from Calibre to the device. The metadata view searches and browses the `metadata.calibre` file that a transfer leaves in the downloads folder. The code lives in `rust/kaia-calibre`, in the Calibre view of `rust/kaia-app/src/surfaces/catalogs/` (`mod.rs`, `cache.rs`, `views.rs`) and in the worker `worker.rs`.

The wireless protocol follows KOReader's Calibre plugin, `plugins/calibre.koplugin/wireless.lua`. The module comment in `rust/kaia-calibre/src/metadata.rs` names KOReader's `metadata.lua` and `search.lua` as references. This spec did not compare the search rules line by line.

## Consent

The Calibre view opens from Network and sync. Nothing on the network happens until the user taps a button.

- Opening the view reads the metadata file in the downloads folder (`Cache::load`, called from `mod.rs`, `build`). It sends nothing.
- Discover sends a UDP "hello" to five broadcast ports. It runs only when the user taps Discover (`mod.rs`, action `discover`). It listens for three seconds (`worker.rs`, `Request::Discover`).
- Connect runs only when the user taps Connect (`mod.rs`, action `connect`). Before it connects, the app saves the address to `calibre.address` (`mod.rs`, `connect`).
- On a Kobo with Wi-Fi off, Discover and Connect use the Wi-Fi prompt of the catalogs screen. Airplane mode blocks them in the same way (see the OPDS spec, Consent).
- Once connected, the server can send books, delete the books it sent, and read the books it sent or that the library indexes. These actions continue until the user taps Disconnect or Cancel, leaves the screen, the app suspends, or the server disconnects. After the connection opens, the status line reads "Connected. Send books from Calibre.".
- Nothing in the Calibre code starts a connection on its own.

## Credentials

- The server password is typed in the field "Server password, optional" (`views.rs`, field `calibre-password`). The screen says "Used for this connection only" (`views.rs`).
- The password is held in memory in the `CatalogSurface` struct (`mod.rs`, field `password`). Connect does not save it (`mod.rs`, `connect` saves only `calibre.address`). It is gone when the app restarts or the screen closes.
- The handshake never sends the password. The reply to `GET_INITIALIZATION_INFO` carries `passwordHash`, the SHA-1 of the password followed by the server's challenge, as hex (`rust/kaia-calibre/src/lib.rs`, `handle`). The hash is empty when no password is set or the server sends no challenge. This is the same digest as KOReader's `wireless.lua` (line 477).
- The address is saved as text in `settings.json` under `calibre.address`.
- The password and the hash are never stored or logged. `Connection` has no `Debug` output, and no code prints the password.

## Data ownership

The downloads folder is `network.downloads`, and it defaults to the books folder (`rust/kaia-app/src/surfaces/catalogs/store.rs`, `downloads`). Every path below sits under that folder.

Reads:

- `metadata.calibre`, or the hidden `.metadata.calibre` when the visible file is missing (`rust/kaia-calibre/src/metadata.rs`, `FILE_NAMES`). The read is capped at 8 MiB (`MAX_BYTES`).
- `metadata.calibre` and `driveinfo.calibre` when a connection starts (`Connection::connect`).
- Book files, but only those in the connection's book list or indexed in the library (`known_book` and `readable_book` in `lib.rs`).

Writes:

- Received books, written to a temporary file in the target folder and renamed when the last byte arrives (`BookStorage::write_file_atomically` in `worker.rs`).
- `metadata.calibre`, after each received book and after each delete batch (`write_metadata_cache` in `lib.rs`). `driveinfo.calibre` is written when the server sends device information (`write_drive_info`).
- If `metadata.calibre` cannot be read at connect time, the damaged bytes are copied to `metadata.calibre.damaged` before the next save replaces the file (`read_metadata_cache` in `lib.rs`).
- The library index in `library.sqlite3`. A received book is indexed by the library scanner (`index_file` in `worker.rs`). A server delete removes the row (`worker.rs`, the `BookDeleted` branch). The library owns this database (`docs/kaia/specs/STORAGE.md`).

Deletes:

- When the server asks to delete a book in its list, Kaia removes the file under the downloads folder (`delete_books` in `lib.rs`, and `BookStorage::delete` in `worker.rs`). Only books in the connection's list count. The list holds the books the server sent in this session or in earlier ones (`uuid` check in `delete_books`).
- A file that is already gone counts as deleted, as KOReader's `removeFile` treats it. A file that cannot be removed stays in the list and in the library. The batch goes on past it (`remove_listed` in `lib.rs`).

Refused names. The server's names must pass `check_lpath` before any read, write or delete (`rust/kaia-calibre/src/policy.rs`). The rules refuse:

- Empty names, absolute names, and names with `.` or `..` parts (`NotRelative`).
- Hidden parts, such as `.kaia`, `.kobo`, `.adds` and dotfiles (`Hidden`).
- Parts ending in `.sdr`, the KOReader sidecar folders (`Sidecar`).
- Backslashes, colons and control characters (`Characters`).
- Driver files `metadata.calibre` and `driveinfo.calibre` as book names, and names whose extension is not in `BOOK_EXTENSIONS` (`NotBook`).

Links are refused at every part of the path below the root (`reject_links`). An existing file at a target path must be a regular file and must be in the book list or indexed. A new name is allowed (`upload_target`).

Never touched: the settings files, the Kobo database, KOReader sidecars, hidden folders, and the statistics database.

## Cancellation and timeouts

Timeouts:

- Connect: 5 s (`TcpStream::connect_timeout`, `Connection::connect`). Writes: 5 s. Reads poll every 250 ms.
- Discovery: the listen window is three seconds and the socket polls every 100 ms (`rust/kaia-calibre/src/lib.rs`, `try_discover_cancelled`).
- There is no idle timeout while connected. A server that stops sending keeps the session open until the user cancels or the connection closes.

Cancellation:

- Cancel and Disconnect send `catalogs:cancel`, which sets the worker's flag (`mod.rs`, action `cancel`). Leaving the screen, suspend and save set the flag too (`mod.rs`, `on_hide` and `on_event`).
- Between messages, the read loop checks the flag on each 250 ms poll (`read_byte`, `read_message`). During a book transfer, the reader checks it on each read (`CancelReader`).
- Discovery stops at the next 100 ms poll.
- The connect step does not check the flag during the 5 s connect timeout. The flag is checked again once the socket is open.

State left behind:

- A book that is cancelled during the transfer has no file. The temporary file is dropped without being renamed (`write_file_atomically`). A test covers this (`calibre_cancel_removes_incomplete_book`).
- Books received before the cancel stay in the downloads folder and in `metadata.calibre`. Each one is recorded after its last byte.
- After a delete, the file and the book-list entry go, but the library row goes only when the worker receives the `BookDeleted` event. A file that cannot be removed keeps its list entry and its library row (see Failures).

## Failures

The screen shows these messages (`worker.rs`, `worker_text`, and `catalogs/mod.rs`):

| Failure | Text id |
|---|---|
| Empty address on Connect | "Enter the server address and port." |
| Address does not parse | "Use a server address and port." |
| Address has no socket address | "Cannot find this server." |
| TCP connect fails or times out | "Cannot connect to Calibre." |
| Session ends with an I/O error or an unparseable message | "Calibre connection ended. Check the server and password." |
| Server sends a book the policy refuses (`PermissionDenied`) | "Calibre sent a file this app does not accept." |
| Server sends a message or an error | "Calibre sent a message. Check the server." |
| Connected | "Connected. Send books from Calibre." |
| Book received | "Book received. Connected to Calibre." |
| Book indexed into the library | "Adding book to the library..." while indexing, then "Book received. Connected to Calibre." |
| A received book cannot be indexed | The file stays and the session goes on. The index message, such as "The book format is not supported.", is the status line until the next book. It is the session summary when the session ends normally. The error is logged on stderr. |
| A listed book cannot be deleted | "Cannot delete this book. It stays on the device." The batch goes on. The message is the session summary when the session ends normally. Calibre still gets an OK for that name (see Known gaps). |

A session that ends with an error, such as a malformed message or a lost connection, shows that error. The summary of earlier problems is not shown then.
| Discovery fails | "Cannot discover Calibre. Try a manual address." |
| Discovery finds nothing, or the session ends normally | "No active connection. Use a manual address or discover servers." or "Choose a server to connect." |
| Metadata file cannot be read | "Cannot read the Calibre metadata file." |
| Metadata file is larger than 8 MiB | "The Calibre metadata file is too large to search." |
| Search with a blank query | "Enter a word to search." |
| Search or browse finds nothing | "No books found." and "No values in the Calibre metadata file." |
| Tapped book is not in the library | "This book is not on the device." |
| Wi-Fi off, airplane mode off | "Wi-Fi is off. Turn it on to continue?" |
| Wi-Fi off, airplane mode on | "Airplane mode is on. Turn it off to use Wi-Fi." |

Protocol refusals that go back to the server (`policy.rs`, `Refusal::message`, and `lib.rs`):

- "The file name is not allowed." for a bad name.
- "The file type is not supported." for a name with a non-book extension.
- "Kaia does not replace this file." for an existing target that is not a known book.
- "No space left on device" when the free space is less than the book plus 128 KiB. Calibre 4.18 and later receive this as an error and no book bytes are read. Older Calibre receive an OK, and Kaia reads and drops the bytes.

## Compatibility

Protocol:

- Opcodes 0 to 20 match `OPCODES` in KOReader's `wireless.lua` (lines 35 to 54). The Kaia constants are in `rust/kaia-calibre/src/lib.rs`, `opcode`.
- Discovery uses the same five broadcast ports as KOReader, 54982, 48123, 39001, 44044 and 59678 (`wireless.lua` line 30). The companion port is 8134 (`wireless.lua` line 32). The app binds UDP port 8134 for discovery replies.
- A discovery reply must start with `calibre wireless device client (on`, followed by the server name, the OPDS port and the socket port (`parse_discovery`).
- The handshake says `appName` is `KOReader` and `deviceName` is `KOReader (Kobo)`. Calibre therefore shows the device as a KOReader device. The version string is `Kaia Reader`.
- `cacheUsesLpaths` is true, as in KOReader (`wireless.lua` line 486). The accepted formats are the list in `DeviceInfo::default`, which the test `advertised_formats_are_the_accepted_book_formats` keeps equal to `BOOK_EXTENSIONS`.
- Calibre 4.18 and later get the error-first refusal, as in `wireless.lua` line 639.

Metadata file:

- The cache uses the fields in `FIELDS` (`lib.rs`). `metadata_cache_matches_koreader_fields` checks them.
- Search and browse read the title, authors, series, tags and publisher. Search ignores case by default. The browse views group books by author, series and tag.

Known gaps:

- A file that cannot be removed gets an OK with its uuid, the same reply as a removed book. KOReader's `deleteBook` replies this way and never sends an error, so Kaia does too. Calibre cannot tell the two apart for that session. The book is still listed, so the next connection offers it again.
- No idle timeout while connected. A silent server keeps the screen on "Connected. Send books from Calibre." until the user cancels.
- No cancel during the 5 s connect timeout.
- Discovery trusts any reply with the right prefix, and the user picks the server. Kaia does not check the server's identity.
- The server password is not saved. Each connection needs it again.
- An unknown opcode ends the session (`handle`, the last arm).
- The protocol has been tested with Kaia's in-process peers and the app's scripted server (`rust/kaia-app/tests/catalogs.rs`, `calibre_send` and `calibre_reply`). It has not been tested against a real Calibre server. `docs/kaia/STATUS.md` lists a Calibre connection from the Kobo under R6.

## Tests

Each contract has at least one row. "No test" means no existing test checks the contract.

| Contract | Test | File |
|---|---|---|
| Opening the Calibre view sends nothing | No test | |
| Discover runs only on a tap | No test | |
| Discover finds a loopback server | `discovery_on_loopback` | `rust/kaia-calibre/src/lib.rs` |
| Discover stops when cancelled | No test | |
| Wi-Fi prompt before a Calibre request on a Kobo with Wi-Fi off | `wifi_prompt_and_back_send_no_requests` | `rust/kaia-app/tests/catalogs.rs` (the shared request path; Calibre Connect is not tested) |
| Airplane mode blocks Calibre requests | `catalogs_do_not_offer_to_turn_wifi_on_in_airplane_mode` | `rust/kaia-app/tests/settings_extras.rs` (the shared request path) |
| Connect saves the address and not the password | No test | |
| Handshake sends the SHA-1 of the password and challenge | `initialization_and_password` | `rust/kaia-calibre/src/lib.rs` |
| Empty password sends an empty hash | `initialization_and_password` | `rust/kaia-calibre/src/lib.rs` |
| The password is not saved or logged | No test | |
| Hidden, sidecar and driver names are refused and create nothing | `hidden_sidecar_and_driver_names_are_refused_and_create_nothing` and `lpath_table` | `rust/kaia-app/tests/calibre_policy.rs`, `rust/kaia-calibre/src/policy.rs` |
| Settings files are neither read nor replaced | `settings_file_is_neither_read_nor_replaced` and `settings_are_neither_read_nor_replaced` | `rust/kaia-calibre/src/lib.rs`, `rust/kaia-app/tests/calibre_policy.rs` |
| The Kobo database is not read or served | `kobo_database_is_not_read` and `kobo_database_is_not_served` | `rust/kaia-calibre/src/lib.rs`, `rust/kaia-app/tests/calibre_policy.rs` |
| Links in the book path are refused | `links_in_the_book_path_are_refused` and `links_in_the_book_path_are_not_followed` | `rust/kaia-calibre/src/lib.rs`, `rust/kaia-app/tests/calibre_policy.rs` |
| Unlisted books are not read | `unlisted_book_is_not_read` | `rust/kaia-calibre/src/lib.rs` |
| Unlisted books are not replaced | `unlisted_book_is_not_replaced` | `rust/kaia-calibre/src/lib.rs` |
| Metadata alone does not list a book | `metadata_alone_does_not_list_a_book` | `rust/kaia-calibre/src/lib.rs` |
| Metadata cannot list another file | `book_metadata_cannot_list_another_file` | `rust/kaia-calibre/src/lib.rs` |
| Executables and scripts are refused before any write | `executables_and_scripts_are_refused_before_any_write` (both files) | `rust/kaia-calibre/src/lib.rs`, `rust/kaia-app/tests/calibre_policy.rs` |
| Indexed books are read and replaced; unindexed files are not | `indexed_books_are_read_and_replaced_and_unindexed_files_are_not` | `rust/kaia-app/tests/calibre_policy.rs` |
| A received book is written, listed and recorded | `booklist_receive_delete_and_space` and `book_upload_and_resend_both_work` | `rust/kaia-calibre/src/lib.rs` |
| A received book is indexed and the screen follows the session | `calibre_screen_receives_book_and_metadata_then_disconnects` | `rust/kaia-app/tests/catalogs.rs` |
| An EPUB can be sent again after a first send | `epub_upload_and_resend_still_work` | `rust/kaia-app/tests/calibre_policy.rs` |
| Plain text keeps its UTF-8 bytes | `plain_text_upload_preserves_utf8_bytes` | `rust/kaia-calibre/src/lib.rs` |
| A server delete removes the file | `booklist_receive_delete_and_space` (checks the file is gone) | `rust/kaia-calibre/src/lib.rs` |
| A server delete removes the list entry | `delete_batch_with_a_missing_file_removes_the_others` | `rust/kaia-calibre/src/lib.rs` |
| A server delete removes the library row | `delete_batch_with_a_missing_file_removes_the_library_rows` | `rust/kaia-app/tests/calibre_policy.rs` |
| A delete of a missing file does not end the session | `delete_batch_with_a_missing_file_removes_the_others` and `delete_batch_with_a_missing_file_removes_the_library_rows` | `rust/kaia-calibre/src/lib.rs`, `rust/kaia-app/tests/calibre_policy.rs` |
| A delete that cannot remove a file keeps it, goes on and is reported | `delete_batch_with_a_permission_error_keeps_the_file_and_goes_on` and `a_book_that_cannot_be_deleted_stays_and_the_summary_says_so` | `rust/kaia-calibre/src/lib.rs`, `rust/kaia-app/tests/calibre_policy.rs` |
| A name the list does not hold gets an OK and no file is touched | `a_name_the_list_does_not_hold_gets_an_ok_and_no_file_is_touched` | `rust/kaia-calibre/src/lib.rs` |
| A damaged metadata cache starts empty and keeps a copy | `a_damaged_metadata_cache_starts_empty_and_keeps_a_copy` | `rust/kaia-calibre/src/lib.rs` |
| The metadata file is read with a size cap, and a directory is refused | `load_refuses_large_files_without_reading_them` and `load_refuses_a_directory_in_place_of_the_file` | `rust/kaia-calibre/src/metadata.rs` |
| The visible or hidden metadata file is read | `load_reads_the_visible_or_hidden_file` | `rust/kaia-calibre/src/metadata.rs` |
| A damaged metadata file is reported | `load_reports_a_damaged_file` and `damaged_json_is_an_error_not_a_panic` | `rust/kaia-calibre/src/metadata.rs` |
| Deeply nested JSON is refused | `deeply_nested_json_is_refused` | `rust/kaia-calibre/src/metadata.rs` |
| An empty metadata file holds no books | `empty_file_has_no_books` | `rust/kaia-calibre/src/metadata.rs` |
| Cancel stops waiting for the server | `cancellation_stops_waiting_for_server` | `rust/kaia-calibre/src/lib.rs` |
| Cancel during a transfer leaves no partial book | `calibre_cancel_removes_incomplete_book` | `rust/kaia-app/tests/catalogs.rs` |
| Books received before a cancel stay | No test | |
| Connect timeout of 5 s | No test | |
| Idle timeout while connected | No test. None exists. | |
| A refused book on a new Calibre gets an error and no bytes | `refused_book_on_new_calibre_gets_error_and_no_bytes` | `rust/kaia-calibre/src/lib.rs` (includes the no-space case, free space 0) |
| A refused book on an old Calibre is read and dropped | `refused_book_on_old_calibre_is_read_and_dropped` | `rust/kaia-calibre/src/lib.rs` |
| A short book is an error | `refused_book_cut_short_is_an_error` | `rust/kaia-calibre/src/lib.rs` |
| A malformed message ends the session | `malformed_message_and_disconnect` | `rust/kaia-calibre/src/lib.rs` |
| Device information messages and file segments | `device_info_messages_and_file_segment` | `rust/kaia-calibre/src/lib.rs` |
| An index error on a received book keeps the file and the session | `an_index_failure_keeps_the_calibre_session_going` | `rust/kaia-app/tests/calibre_policy.rs` |
| Unknown opcode | No test | |
| Calibre error text mapping | No test | |
| Opcode numbers match KOReader | No test. Checked by reading the two tables. | |
| Accepted formats match the advertised list | `advertised_formats_are_the_accepted_book_formats` | `rust/kaia-calibre/src/lib.rs` |
| Metadata fields match KOReader | `metadata_cache_matches_koreader_fields` | `rust/kaia-calibre/src/lib.rs` |
| Search is case-insensitive for non-ASCII text | `search_is_case_insensitive_for_non_ascii` | `rust/kaia-calibre/src/metadata.rs` |
| Search matches all words across fields | `search_matches_all_words_across_fields` | `rust/kaia-calibre/src/metadata.rs` |
| Search finds series, tags and publisher | `search_finds_series_tags_and_publisher` | `rust/kaia-calibre/src/metadata.rs` |
| Search switches turn fields off | `search_switches_turn_fields_off` | `rust/kaia-calibre/src/metadata.rs` |
| Case-sensitive search needs the same case | `case_sensitive_search_needs_the_same_case` | `rust/kaia-calibre/src/metadata.rs` |
| Browse groups count each book once per value | `groups_count_books_once_per_value` and `books_with_matches_the_value_exactly` | `rust/kaia-calibre/src/metadata.rs` |
| Metadata entries without a safe path are skipped | `parses_fields_and_skips_unusable_entries` | `rust/kaia-calibre/src/metadata.rs` |
| Real Calibre server | No test | |
