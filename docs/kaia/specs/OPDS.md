# OPDS catalog spec

Status: version 1. Describes the code as of 2026-10-14. The catalog client has been tested only against local test servers.

This spec covers OPDS catalog browsing, search and downloads in the Rust app. It covers the `kaia-opds` crate, the catalogs screen in `rust/kaia-app/src/surfaces/catalogs/`, and the book download path in `worker.rs`.

The crate ports KOReader's `plugins/opds.koplugin`. `opdsbrowser.lua` defines the request headers and redirect rules, and `main.lua` defines the default catalog list.

## Consent

No catalog is active until the user adds it.

- The Suggestions screen offers KOReader's six default catalogs (`rust/kaia-opds/src/catalog.rs`, `default_catalogs`). A suggestion is saved only when the user taps Save (`rust/kaia-app/src/surfaces/catalogs/mod.rs`, action `save`).
- Opening the catalogs screen sends nothing. Browsing, searching, opening a book's details and downloading each send requests.
- Opening a book's details also fetches its cover. The request starts when the book view is built (`mod.rs`, `build`). This happens without a further tap.
- On a Kobo with Wi-Fi off, a request does not start. The screen shows "Wi-Fi is off. Turn it on to continue?" with a "Turn on Wi-Fi" button (`mod.rs`, `request`, and the `catalogs:wifi` action). Wi-Fi turns on only after that tap.
- Airplane mode blocks these requests on a Kobo with Wi-Fi off. The screen shows "Airplane mode is on. Turn it off to use Wi-Fi." and sends nothing (`mod.rs`, `request`). The `catalogs:wifi` action checks airplane mode again before it starts Wi-Fi. The check runs only in the Kobo branch of `request`, so desktop builds ignore airplane mode for catalogs.
- Kaia does not turn Wi-Fi off after a catalog request. The catalog code has no call that turns Wi-Fi off.

## Credentials

Catalog credentials are the username and password that the user types in the catalog editor.

- Stored in `settings.json` under `opds.catalogs`, as the `title`, `url`, `username` and `password` fields in plain text (`rust/kaia-app/src/surfaces/catalogs/store.rs`, `save`). The editor says "Stored on this device without encryption" (`views.rs`, the editor text).
- Sent as HTTP Basic authentication. The header is `Basic` followed by the base64 of `username:password` (`rust/kaia-opds/src/client.rs`, `Client::for_catalog`).
- Sent only to the catalog's origin: scheme, host and port (`authorization_for` in `client.rs`, `Origin::of` in `resolve.rs`). A redirect to another origin drops the header. A redirect within the origin keeps it. A redirect from https to http is refused (`redirect_target`).
- Over http the header travels unencrypted. The default catalog ManyBooks uses `http://` (`catalog.rs`).
- An address with a username or password in its URL is refused by the editor (`valid_url` in `mod.rs`).
- The password field is masked in the editor (`views.rs`, `text_field` with the password flag).
- `Catalog` prints `<hidden>` for the password in its `Debug` output (`catalog.rs`).
- The catalog password is separate from the Calibre server password, which is never saved (see the Calibre spec).
- Kavita's API key is part of the page URL and goes in the query string of its login request. The app never calls that code (see Compatibility).

The password is stored only in `settings.json`. The Authorization header and the password are never logged, and no code path prints a catalog password.

## Data ownership

Reads:

- `opds.catalogs` and `network.downloads` in `settings.json`.
- The downloads folder, to pick a name that is not taken (`publish_download` in `worker.rs`).

Writes:

- `opds.catalogs` and `network.downloads` in `settings.json`. The file is flushed when the editor saves or deletes a catalog, and when the downloads folder changes (`mod.rs`, `flush`).
- Downloaded books, in the downloads folder. The default is the books folder (`store::downloads`). The client writes each download to a hidden `.<name>.part` file first, then renames it to its final name (`rust/kaia-opds/src/client.rs`, `download_as`). The worker downloads into a temporary folder inside the downloads folder and moves the finished file out (`worker.rs`, `publish_download`).
- Each finished book is indexed into `library.sqlite3` through the library scanner (`worker.rs`, `index_file`). The library owns that database (`docs/kaia/specs/STORAGE.md`).

The downloads folder receives only the files the reader can open (`files.rs`, the `PROVIDERS` table). Covers are decoded into memory and never written to disk.

Never touched: KOReader sidecars (`.sdr` folders and docsettings), the statistics database, book state and the Kobo database.

## Cancellation and timeouts

Timeouts (`rust/kaia-opds/src/client.rs`, the constants at the top and `agent`):

- Feeds, search descriptions and covers: 10 s each for connecting, sending and waiting for the response. The body has no limit of its own, so the whole call is limited to 30 s.
- Downloads: 15 s per socket phase. The body has a 30 minute limit and no whole-call limit.
- Redirects: 5 at most (`MAX_REDIRECTS`). A 3xx reply with no `Location` header is a status error.

Size limits:

- Feed body: 16 MiB (`MAX_FEED`).
- Cover or page image: 32 MiB (`MAX_IMAGE`).
- Download: 512 MiB by default (`DEFAULT_MAX_DOWNLOAD`). The app does not change the limit. The limit is checked against `Content-Length` before the body is read, and again while the body is written.

Cancellation:

- The Cancel button and the Disconnect button send `catalogs:cancel`. The status then reads "Cancelling...". The catalog worker stops at the next check (`mod.rs`, action `cancel`).
- Leaving the screen drops the worker, and dropping it sets the cancel flag (`mod.rs`, `on_hide`; `worker.rs`, `Drop for Worker`).
- Suspend and save set the flag too (`mod.rs`, `on_event`).
- The client checks the flag before each request, between body reads, and while it waits on a socket. The socket wait wakes every 100 ms (`rust/kaia-opds/src/cancel.rs`).
- After a cancel, the screen ignores all replies except the last one, which shows "Cancelled.".

State left behind:

- A cancelled or failed download removes its `.part` file (`download_as`). The temporary folder is removed when the worker drops it.
- A download that finished before the cancel stays in the downloads folder. The worker indexes a published file even when the user has left the screen (`worker.rs`, the comment on the index step).
- If the library cannot index a published file, the file stays in the downloads folder and the screen shows "The book was saved but could not be indexed." or "The book format is not supported.".
- The catalog list and the settings file keep their values after a cancel.

## Failures

The worker maps each `kaia_opds::Error` to one message (`rust/kaia-app/src/surfaces/catalogs/worker.rs`, `opds_error`):

| Error | Text id |
|---|---|
| `Unauthorized`, `Forbidden` | "Authentication failed. Check the username and password." |
| `Parse` | "This address did not return an OPDS feed." |
| `Cancelled` | "Cancelled." |
| `Timeout` | "The server took too long to reply." |
| `Io` | "Cannot save the book. Check the downloads folder." |
| `NotFound` | "The server could not find this item." |
| `Indirect` | "This format needs an external download service." |
| Every other error, including status, network, too large, downgrade, redirect and invalid URL | "Cannot load this item. Check the address and connection." |

Other failures on the same screen:

- A book with no supported download format: "No supported download format." (`views.rs`).
- An indirect acquisition with no matching format: "The catalog did not offer this download format." (`worker.rs`).
- A cover that cannot be decoded: "Cannot read this cover."
- Wi-Fi: "Cannot turn on Wi-Fi." and "Cannot join the saved Wi-Fi network."
- Downloads folder: "Cannot create the downloads folder.", "Cannot prepare the download.", "Cannot save the downloaded book." and "Too many books share this filename."
- Editor: "Enter a name and an HTTP or HTTPS address." when the name is blank or the address is not a valid http or https URL.
- Downloads folder setting: "Enter an absolute folder path." when the path is relative.
- A save or delete that works: "Catalog saved." and "Catalog deleted."

## Compatibility

Feeds:

- OPDS 1.x (Atom) and OPDS 2.0 (JSON) feeds parse through `parse_feed` (`rust/kaia-opds/src/parse.rs`). The format is chosen from the first byte of the body.
- OpenSearch descriptions give the Atom search template. The search screen reads the template from the feed or from the description document (`Client::search`).
- Pagination follows the `next` and `previous` links.
- Feeds send `Accept: application/opds+json, application/atom+xml;profile=opds-catalog, */*`. Search descriptions, covers and downloads send `Accept: */*`. Every request sends `accept-encoding: identity`, as KOReader does (`client.rs`, the constants).

Downloads:

- The file types come from the table in `rust/kaia-opds/src/files.rs`. The table lists the extension and MIME pairs that KOReader registers for its document engines. An acquisition is offered when its URL extension or its MIME type is in the table (`Entry::downloads` in `model.rs`).
- File names come from the book name, the `Content-Disposition` header or the last path segment, and they follow KOReader's `getSafeFilename` rules (`files.rs`).

Default catalogs:

- The six default catalogs match KOReader's list in `plugins/opds.koplugin/main.lua`, lines 25 to 45, by title and URL.

Known gaps:

- Basic authentication is the only scheme. Digest, OAuth and client certificates are not supported.
- OPDS 1.x and OPDS 2.0 both keep `buy` and `sample` acquisitions. Both skip `borrow` (`rust/kaia-opds/src/opds1.rs`, `opds2.rs`). A `buy` link with a file type the table knows, such as `text/html` (which maps to `.htm`), appears as a download. No test covers this.
- The page-streaming (PSE) and Kavita last-page code in `client.rs` and `model.rs` is in the crate but the app never calls it. The Kavita lookup sends the API key in the query string of its login request.
- The app tests only against local test servers (`rust/kaia-opds/tests/support/mod.rs`, `rust/kaia-app/tests/catalogs.rs`). The default catalogs have not been tested against their live servers.
- The default ManyBooks catalog uses plain http.
- The cover request uses the feed client, so it has the 30 s whole-call limit and no separate limit.

## Tests

Each contract has at least one row. "No test" means no existing test checks the contract.

| Contract | Test | File |
|---|---|---|
| Opening the catalogs screen sends nothing | `wifi_prompt_and_back_send_no_requests` | `rust/kaia-app/tests/catalogs.rs` (asserts zero requests before browsing and after Back) |
| Suggestions are saved only on Save | `catalog_editor_suggestions_and_plain_password_persist` | `rust/kaia-app/tests/catalogs.rs` (asserts one catalog after Save) |
| Default catalogs are not active until added | No test | |
| Opening a book's details fetches its cover without a tap | No test | |
| Wi-Fi prompt before the first request on a Kobo with Wi-Fi off | `wifi_prompt_and_back_send_no_requests` | `rust/kaia-app/tests/catalogs.rs` |
| Airplane mode blocks catalog requests | `catalogs_do_not_offer_to_turn_wifi_on_in_airplane_mode` | `rust/kaia-app/tests/settings_extras.rs` |
| Turning Wi-Fi on happens only after the user taps the prompt | `catalogs_offer_to_turn_wifi_on_without_airplane_mode` and `wifi_prompt_and_back_send_no_requests` | `rust/kaia-app/tests/settings_extras.rs`, `rust/kaia-app/tests/catalogs.rs` |
| Kaia does not turn Wi-Fi off after a request | No test | |
| Credentials are sent to the catalog origin only | `basic_auth_is_sent_only_with_the_catalog_credentials` and `authorization_only_for_the_catalog_origin` | `rust/kaia-opds/tests/http.rs`, `rust/kaia-opds/src/client.rs` |
| Same-origin redirect keeps the credentials | `same_origin_redirects_keep_the_credentials` | `rust/kaia-opds/tests/http.rs` |
| Cross-origin redirect drops the credentials | `credentials_do_not_follow_a_redirect_to_another_origin` | `rust/kaia-opds/tests/http.rs` |
| Port change drops the credentials | `authorization_only_for_the_catalog_origin` | `rust/kaia-opds/src/client.rs` |
| `Debug` output hides the password | `debug_output_hides_the_password` | `rust/kaia-opds/src/catalog.rs` |
| The catalog password is saved in plain text | `catalog_editor_suggestions_and_plain_password_persist` (asserts the saved value) | `rust/kaia-app/tests/catalogs.rs` |
| A URL with a username or password is refused | No test | |
| The password never reaches a log | No test | |
| Downloads land in the downloads folder under the server's name | `download_writes_the_suggested_name` and `download_as_takes_the_name_from_the_server_when_none_is_given` | `rust/kaia-opds/tests/http.rs` |
| Downloads are indexed and existing files are kept | `indirect_downloads_are_indexed_and_keep_existing_files` | `rust/kaia-app/tests/catalogs.rs` |
| Relative downloads folder is refused | No test | |
| Book file names follow the plugin's rules | `book_names_follow_the_plugin`, `long_names_keep_the_extension`, `long_multibyte_names_cut_on_a_char_boundary` and `empty_book_name_gets_a_fallback` | `rust/kaia-opds/src/files.rs` |
| Cancelling a download leaves no partial file | `a_cancelled_download_leaves_no_file_behind` | `rust/kaia-opds/tests/http.rs` |
| Cancelling returns without waiting for a stalled server | `stalled_feed_cover_and_download_cancel_without_waiting_for_server` | `rust/kaia-app/tests/catalogs.rs` |
| Cancelling a feed shows "Cancelled." promptly | `bad_feed_auth_failure_and_cancel_use_local_server` | `rust/kaia-app/tests/catalogs.rs` |
| Leaving the screen or suspending cancels | No test | |
| Feed and cover time limits | No test | |
| Socket time limits for downloads | No test | |
| Download size limit is enforced | `downloads_over_the_size_limit_are_refused` | `rust/kaia-opds/tests/http.rs` (uses a 100-byte limit; the 512 MiB default is not tested) |
| Feed and cover size limits | No test | |
| Redirect limit | `redirect_loops_stop` | `rust/kaia-opds/tests/http.rs` |
| https to http redirect is refused | `redirects_refuse_a_downgrade` | `rust/kaia-opds/src/client.rs` |
| Status codes map to their error variants | `http_errors_map_to_their_variants` | `rust/kaia-opds/tests/http.rs` |
| Only http and https addresses are read | `only_http_and_https_are_read` | `rust/kaia-opds/tests/http.rs` |
| Bad base addresses are refused | `bad_base_addresses_are_rejected` | `rust/kaia-opds/tests/parse.rs` |
| Malformed feed is an error, not a panic | `malformed_feed_is_an_error_not_a_panic` | `rust/kaia-opds/tests/parse.rs` |
| A feed with no search says so | `a_feed_without_search_says_so` | `rust/kaia-opds/tests/http.rs` |
| Indirect acquisitions resolve to their files | `indirect_acquisitions_resolve_to_their_files` | `rust/kaia-opds/tests/http.rs` |
| Timeout is reported as "The server took too long to reply." | No test | |
| A body that ends early, or passes the size limit mid-body, leaves no partial file | `a_failed_body_leaves_no_partial_file` | `rust/kaia-opds/tests/http.rs` |
| `opds_error` text mapping | No test | |
| Request headers match `opdsbrowser.lua` | `fetch_sends_the_opds_headers_and_resolves_links` | `rust/kaia-opds/tests/http.rs` |
| Atom and OPDS 2.0 parsing | `navigation_feed_lists_sub_catalogs_in_order`, `acquisition_feed_offers_each_format_with_its_file_type`, `opds2_feed_is_read_in_the_plugin_order` | `rust/kaia-opds/tests/parse.rs` |
| Relative links resolve against the feed URL | `relative_links_resolve_against_the_feed_url` | `rust/kaia-opds/tests/parse.rs` |
| Pagination | `pagination_follows_next_and_previous_links` | `rust/kaia-opds/tests/http.rs` |
| Search through a template and through an OpenSearch description | `search_through_a_template_url_encodes_the_terms` and `search_through_an_opensearch_description` | `rust/kaia-opds/tests/http.rs` |
| PSE page links and page fetch | `pse_entries_carry_the_page_template_and_last_page` and `pse_pages_are_fetched_from_the_template` | `rust/kaia-opds/tests/parse.rs`, `rust/kaia-opds/tests/http.rs` |
| Kavita last page | `kavita_reports_the_last_page_read` and `kavita_fields_come_from_the_page_url` | `rust/kaia-opds/tests/http.rs`, `rust/kaia-opds/src/client.rs` |
| Default catalogs match KOReader | `defaults_match_the_plugin` | `rust/kaia-opds/src/catalog.rs` |
| File types from the table | `file_type_prefers_the_url_extension`, `file_type_falls_back_to_mime` and `unknown_types_are_not_offered` | `rust/kaia-opds/src/files.rs` |
| `buy` and `borrow` relations | No test | |
| Real catalog servers | No test | |
