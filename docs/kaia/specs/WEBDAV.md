# WebDAV notes export

Status: implemented for B-120. Local server and headless tests cover the
contract below. A real WebDAV server and a Kobo device still need checks.

## Consent and requests

Settings, Network and sync has an "Export notes to WebDAV" switch, off by
default. The server URL, username and folder are `webdav.server`,
`webdav.username` and `webdav.folder` in `settings.json`. There is no default
server. The password field starts empty and writes a separate credential file.

With the switch on, Export now queues saved notes for all books. The existing
Notes export actions do the same alongside their local exports. Closing a book
queues that book after its reading state has saved. A book with no notes is
skipped unless it has an earlier export or pending upload. Removing the last
note replaces the remote export with an empty annotations list.

Test connection requires the switch, Wi-Fi and airplane mode off. It sends one
`PROPFIND` with `Depth: 0` to the chosen folder and ignores the response body.
An absent folder can return 404 here. Export creates the folder with `MKCOL`.
Nested folder components are created in order, below the server URL's path.
An existing collection's 405 response is accepted. Each upload then sends two
`PUT` requests. No code path sends GET or DELETE, imports remote annotations,
or reads a remote response body.

Production accepts HTTPS only and verifies TLS certificates. The progress sync
option for self-signed certificates does not affect WebDAV. URLs containing
credentials, queries or fragments are refused. Folder components cannot be
`.` or `..`. Redirects are disabled, so Basic authentication never follows one.
Tests alone can allow HTTP to a loopback IP address. This switch is compiled
only with `cfg(test)` and is never read from settings.

The client uses `ureq`, as progress sync and OPDS do. Upload calls use sync's
2-second socket phase and 5-second total timeout. Test connection uses its
5-second socket phase and 10-second total timeout. A worker thread handles
requests. Each request checks consent and connectivity again. Turning the
switch off or airplane mode on stops the next request; an in-flight request
can finish or time out. Closing the app cancels unsent requests without waiting
for that thread. The queue already holds the work.

## Files and credentials

The remote names are `<Book title> - <author>.md` and `.json`. Unsafe filename
characters become underscores. Title and author each have a 90-byte limit.
Missing values use "Untitled" and "Unknown author". A name already assigned to
another document receives a document hash suffix. The queue remembers those
assignments. Renaming a book leaves the previous remote files in place, since
Kaia never deletes remote files.

Both formats come from `surfaces/notes/export.rs`, the same formatter as local
export. Each request contains one book. A SHA-256 hash covers both formats with
a fixed export timestamp plus the filename. It therefore changes for notes,
highlights, bookmarks, stickers, ink counts or metadata, but not just because
another export happened later. Successful hashes are scoped to the server,
folder, username and book. The prior hash is invalidated before dispatch, because a failed upload may replace only one file. Both PUTs must succeed before a new completed hash is saved.
A stop between PUTs can leave a mixed pair until the next retry replaces both.

Sync stores an MD5-derived user key and cannot supply a Basic-auth password.
WebDAV stores the real password in `<data>/webdav-password`, as unencrypted
UTF-8 text with Unix mode `0600`. A private temporary file is synced and renamed
into place, then its directory is synced. No password, Basic header or server
response body is logged or copied into `settings.json` or the queue. A blank
password clears the credential's contents. The file is readable by the device
user and by anyone who can bypass filesystem permissions.

`<data>/webdav-queue.json` holds pending Markdown and JSON, filenames, content
hashes, destination identifiers, attempt counts, retry deadlines, the
failure-notified flag, completed hashes and filename assignments. It also
holds a refused-login flag. This single file keeps queue removal and completed
hash updates atomic. It contains private notes but no account password. Writes
use a staged file, fsync and rename. The app's existing data-directory lease
keeps a second app instance from writing. Unreadable JSON is left untouched and
reported. Erase all Kaia data removes these files and cancels unsent work.
Reset settings turns export off but retains the credential and queue files.

## Queue and failures

The queue keeps the latest payload per book, at most 100 books and 8 MiB of
Markdown and JSON text. Full queues show a message and keep existing work.
Airplane mode and no Wi-Fi hold entries without counting an attempt. Kaia does
not turn Wi-Fi on. Pending work checks connectivity at most every five seconds
while the app runs, so a Wi-Fi connection releases due entries.

An upload has at most three attempts. After the first failure it waits 30
seconds, then 120 seconds after the second. Attempts and deadlines persist
before dispatch, so a restart cannot reset the limit. A failure shows once per
queued payload. An exhausted entry stays queued. Export now starts a new
three-attempt allowance; another book close with unchanged notes does not.
Changed content starts a new allowance. A 401 holds all requests across
restarts and shows a clear account error. Only editing the account releases
that hold. An account edit also cancels the old worker's unsent requests.
Changing the destination does not send old queued notes to the new destination;
a new Export or close queues them there.

## Contract tests

Unit tests live in `rust/kaia-app/src/webdav/tests.rs`. They create a local
HTTP server that checks Basic auth and handles PROPFIND, MKCOL and PUT.
The app integration test is `rust/kaia-app/tests/webdav.rs`.

| Contract | Test |
|---|---|
| Both files have sanitized names and the local export content; unchanged notes send nothing across restart; last-note removal replaces the export | `webdav_upload_reuses_local_formats_and_unchanged_notes_send_nothing_after_restart` |
| 401 shows once, holds across restart and resumes after account edit | `webdav_401_reports_once_and_holds_across_restart_until_account_edit` |
| Airplane mode and no Wi-Fi queue without sending; connection releases the queue | `webdav_airplane_and_no_wifi_queue_then_connect_uploads` |
| Default off; off sends nothing, even with a pending queue or Test connection tap | `webdav_off_sends_nothing_even_with_a_pending_queue_and_test_tap` |
| One PROPFIND for Test connection; HTTP refused except loopback in tests | `webdav_test_is_one_propfind_and_http_requires_a_test_only_loopback_switch` |
| Backoff, three-attempt limit and one failure message | `webdav_transient_failure_uses_bounded_backoff_and_reports_once` |
| Credential file mode 0600; no password in settings or queue | `webdav_password_is_private_and_absent_from_queue_and_settings` |
| Settings persist; book close and Export now queue saved notes | `webdav_settings_store_password_separately_and_book_close_queues_saved_notes` |
| Nested folder creation and filename collisions | `webdav_nested_folders_and_duplicate_names_keep_separate_files` |
| A failed second PUT cannot make reverted notes look unchanged | `webdav_partial_upload_then_reverted_notes_replaces_both_files` |
| Corrupt queue preserved; redirects refused | `webdav_bad_queue_stays_untouched_and_redirects_are_not_followed` |
| All WebDAV controls fit the Settings pages | `webdav_network_controls_fit_the_paged_settings_screen` |
