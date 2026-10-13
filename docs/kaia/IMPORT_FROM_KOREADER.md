# Import from KOReader

Status: implemented, tested with the desktop test suite. Not tested on a Kobo.

Kaia has its own data folder, so on first start it has no settings and no reading history. The Import from KOReader step copies the useful parts of stock KOReader's data into Kaia. The copy is one-way and runs once. It reads KOReader's files and never changes them.

## When Kaia offers the import

Kaia looks for KOReader's data folder next to its own:

| Device | Kaia data folder | KOReader data folder |
|---|---|---|
| Kobo | `.adds/kaia` | `.adds/koreader` |
| Desktop (macOS) | `~/Library/Application Support/kaia` | `~/Library/Application Support/koreader` |
| Desktop (Linux) | `$XDG_CONFIG_HOME/kaia` or `~/.config/kaia` | `$XDG_CONFIG_HOME/koreader` or `~/.config/koreader` |

A folder counts only when it contains `settings.reader.lua`.

Kaia offers the import once, when Home opens, if all of these are true: KOReader's folder exists, Kaia's reading history is empty, and the offer has not been shown before. The offer has two buttons, Import and Not now. Kaia records that the offer was shown whichever button you press.

To run the import later, open the file manager menu and go to More tools. Import from KOReader is next to Extensions. The same confirmation appears. Kaia does not offer the import again after the first time, but the menu entry stays available.

## What is copied

Before anything is copied, Kaia shows the plan: the number of settings, the number of books in the reading history, the number of book data files and the names of the plugin files.

| KOReader item | Kaia location | Notes |
|---|---|---|
| `settings.reader.lua` | `settings.reader.lua` | Merged, see below. |
| `history.lua` | `history.lua` | Copied as it is. Only when Kaia has no history file yet. |
| `docsettings/**/*.sdr/` sidecars | `docsettings/**/*.sdr/` | Reading position, highlights, notes, custom cover and custom metadata. |
| `hashdocsettings/**/*.sdr/` sidecars | `hashdocsettings/**/*.sdr/` | Same files, for books that KOReader keeps by file hash. |
| `settings/` plugin data | `settings/` | For example `statistics.sqlite3`, `vocabulary_builder.sqlite3`, `gestures.lua`, `hotkeys.lua`, `profiles.lua`, `calibre.lua`. |
| `styletweaks/` | `styletweaks/` | Style tweak files. |

Plugin settings can hold account logins, for example the KOSync or Wallabag settings. These files are copied as they are, so Kaia signs in to the same account. Change the login in Kaia if you want a different account.

Only files that match these names are copied from the sidecar folders: `metadata.<ext>.lua` and its `.old` backup, `custom_metadata.lua`, and `cover.<ext>`. The sidecar must sit in a `<book>.sdr` folder. Books are never copied.

## What is not copied, and why

| KOReader item | Reason |
|---|---|
| `patches/` | User patches are code. They can change behavior in ways Kaia does not test. The import lists their names so that you can copy a patch by hand if you want it. |
| `plugins/` (user plugins) | Same reason as patches. The names are listed. |
| `settings/bookinfo_cache.sqlite3` | A cache. KOReader rebuilds it, and Kaia rebuilds its own. |
| `settings/kosync_queue.lua` | Reading progress that KOReader has not uploaded yet. Replaying it from Kaia could push an old position to the sync server. |
| `settings/*.old` and SQLite journal files (`-journal`, `-wal`, `-shm`) | Backups and temporary files. |
| `screenshots/`, `cache/`, `ota/`, `clipboard/` | Not user data, or rebuilt on demand. |
| `data/` (dictionaries and other large data) | Not copied in this version. Copy these by hand if you need them. |
| Books | Books stay where they are. The import does not move or copy them. |

Any file larger than 200 MB is skipped. The plan and the result name each skipped file.

## Settings merge

Kaia's settings file is merged with KOReader's settings file. The merge follows these rules:

- Kaia's values always win. The import never changes a setting that Kaia already has.
- KOReader's values fill in keys that Kaia does not have.
- These keys are dropped, because they tie the install to one device or profile: `start_with`, `document_metadata_folder`, `device_id`, `quickstart_shown_version`, and any key starting with `kaia_` or `ota_`.
- `lastdir` and `home_dir` are dropped only when the path they point to does not exist on this device.

Kaia then records the time of the import in `kaia_imported_from_koreader`.

## Files that already exist

The import never replaces a file that Kaia already has, with one exception: the settings file is merged as described above. A file that already exists is left alone and counted in the result. Running the import again copies nothing else, but it merges the settings again.

## KOReader keeps working

The import only reads from KOReader's folder. It does not create, change, rename or delete any file there, and it does not change the mtime of any file there. The import checks this in its tests.

Book sidecars still sit next to each book in KOReader's `.sdr` folder. KOReader keeps reading and writing them. Kaia keeps its own copies in its data folder, so after the import the two apps do not share state. Changes made in one app are not seen by the other.

Close stock KOReader before you run the import. KOReader may hold a SQLite database open, and a copy taken while KOReader writes to it can be inconsistent.

## Undo

- To remove everything the import added, delete Kaia's data folder (`.adds/kaia` on Kobo, or the desktop folder shown above). This also removes Kaia's own settings, stickers and reading data made since then. KOReader's folder is not touched. Kaia offers the import again on its next start, because the offer flag is stored in the same folder.
- To go back to an earlier state, restore a backup made with `tools/kaia/device/backup.sh` (see `docs/kaia/device/BACKUP_AND_RECOVERY.md`). Run `tools/kaia/device/restore.sh` with a dry run first. It writes nothing until you add `--apply`.

Make a backup before you run the import if you already use Kaia.

## Limits

- The import is one-way and runs on demand. Kaia does not sync with KOReader.
- The reading history keeps the file paths that KOReader recorded. These paths are valid on the same device, because both apps read the same book folders.
- Dictionaries, user fonts in Kaia's data folder and user plugins are not copied. Fonts in `/mnt/onboard/fonts` are shared by both apps and need no import.
- DRM-protected books are not supported by either app.
