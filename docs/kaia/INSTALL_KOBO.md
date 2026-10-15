# Install Kaia Reader on a Kobo

Kaia Reader is an open source e-reader for the Kobo Libra Colour. It installs next to stock KOReader and Nickel. It has its own folder, its own settings file and its own launch entry. It does not download KOReader packages, and it hides KOReader's update menu.

Status: tested on one device. The first device run on 2026-10-14 used the 4.x package on a Kobo Libra Colour with firmware 4.42.23303. It checked the install, both NickelMenu entries, the ExcludeSyncFolders step below, and that the Lua and Rust apps start. Its record is in [device/FIRST_DEVICE_RUN.md](device/FIRST_DEVICE_RUN.md). No 5.x run is recorded. The known gaps are near the end of this guide.

## Before you start

1. Back up the Kobo before you use Kaia for a library you care about. Follow [device/BACKUP_AND_RECOVERY.md](device/BACKUP_AND_RECOVERY.md).
2. Check the firmware version. See the next section.
3. Install NickelMenu on the Kobo. NickelMenu is not part of the Kaia package. Install it separately and follow its own documentation. NickelMenu does not support firmware 5.x. For firmware 5.x, see the section "Firmware 5.x" below.
4. Charge the battery.

## Check the firmware version

There are two ways to read the firmware version.

1. On the Kobo, open the settings and find the device information page. It shows the firmware version. Menu names differ between firmware versions.
2. Over USB, open the file `.kobo/version` on the Kobo storage. The third comma-separated field is the firmware version. An example value is `4.38.23171`.

## Choose the package

Kaia builds one package per firmware family.

| Firmware | Package file name | Built from |
|---|---|---|
| 4.x | `kaia-kobo-<version>-<hash>.zip` | `dist/koreader-kobo-*.zip` and `dist/kobo/libkaia.so` |
| 5.x | `kaia-kobo-v5-<version>-<hash>.zip` | `dist/koreader-kobov5-*.zip` and `dist/kobov5/libkaia.so` |

Both packages include the Lua app and an optional Rust app. Choose the package that matches your firmware. The 4.x package ran on a Libra Colour on 2026-10-14 (`device/FIRST_DEVICE_RUN.md`). No 5.x run is recorded.

## Check the download

Each GitHub release has both packages, `SHA256SUMS` and `SHA256SUMS.sig`. The signature is made with the Kaia release key. It shows that the project owner published the sums. The sums show that each package is the file that was signed.

1. Make a new empty folder. Save the package you chose, `SHA256SUMS` and `SHA256SUMS.sig` in it.
2. Get the Kaia source at the tag of that release, for example with `git clone` and `git checkout` of the tag.
3. From that checkout, run the check. This example uses a folder named `kaia-release` in Downloads:

   ```sh
   sh tools/kaia/release/verify-release.sh ~/Downloads/kaia-release
   ```

   A pass prints `Good "kaia-release" signature` with the key fingerprint, one `OK` line for each file, and `OK: the signature and every file match SHA256SUMS.` The script stops with a message when a check fails. Do not install a package that fails.

To run the checks by hand, go to the folder and run these commands. Use the path to `docs/kaia/release/allowed_signers` in the checkout:

```sh
ssh-keygen -Y verify -f ~/kaia-reader/docs/kaia/release/allowed_signers -I release@kaia-reader -n kaia-release -s SHA256SUMS.sig < SHA256SUMS
shasum -a 256 -c SHA256SUMS
```

The release key is not created yet. Until the owner creates it and commits `docs/kaia/release/allowed_signers`, the check stops and says that no key is recorded. When a key exists, compare the fingerprint that the check prints with the one recorded for B-102 in `docs/kaia/DECISIONS_NEEDED.md`.

## Install

1. Connect the Kobo to the computer with USB. Wait until the storage mounts.
2. Unzip the package at the root of the Kobo storage. The zip contains `.adds/kaia/` and `.adds/nm/kaia`. Let the file manager merge the `.adds` folder with the existing one. Do not delete `.adds/koreader`.
3. Eject the Kobo in the file manager, then unplug the cable.
4. Start Kaia from NickelMenu. Choose "Kaia Reader" for the Lua app, or "Kaia Reader Rust" for the experimental Rust app.

The entry file `.adds/nm/kaia` holds both NickelMenu entries. Its syntax was checked against NickelMenu v0.6.0 on a Libra Colour with firmware 4.42.23303 on 2026-10-14, and both entries start Kaia.

## Stop Nickel from listing Kaia's files as books

Nickel scans every folder on the storage for books, hidden ones included. After the install, it lists Kaia's icons and help pages as books. KOReader has the same problem and the same fix. Do this once after the install.

1. Connect the Kobo over USB.
2. Open `.kobo/Kobo/Kobo eReader.conf` in a plain text editor. Find the `[FeatureSettings]` section. If the file has none, add it at the end of the file.
3. Add this line under `[FeatureSettings]`:

```
ExcludeSyncFolders=(\\.(?!kobo|adobe).+|([^.][^/]*/)+\\..+)
```

4. Save the file. Eject the Kobo, then unplug the cable.

Nickel then ignores folders whose names start with a dot. Edit the file only while the Kobo is in USB mode, and keep a copy of it first.

On 2026-10-14 the first device run added this line, then did a full library rescan from NickelMenu (`nickel_misc:rescan_books_full`). Nickel listed the 3 test books instead of 129.

## Firmware 5.x

NickelMenu states that it does not support firmware 5.x. On firmware 5.x, start Kaia with another launcher, such as KFMon, and follow that launcher's documentation. The Kaia launcher reads `.adds/kfmon/config/kaia.ini` when it exits under KFMon. If that file contains the line `reboot_on_exit=false`, the launcher restarts Nickel. Without that line, the launcher reboots the device.

For the Rust app on firmware 5.x, point the launcher at `/mnt/onboard/.adds/kaia/kaia-rust.sh`.

Do not put a 5.x binary on 4.x firmware. No 5.x run is recorded, and the KFMon route is not verified.

## Start the Lua app

"Kaia Reader" is the default app. It runs KOReader's frontend with Kaia's Home, Library, Notes and Settings screens. Choose "Kaia Reader" in NickelMenu.

The first device run on 2026-10-14 checked that the Lua app starts, opens EPUB and PDF files, turns pages by touch and buttons, sleeps and wakes, and exits to Nickel from the menu.

## Start the Rust app

Kaia Reader Rust is experimental. Read [device/RUST_APP_TEST.md](device/RUST_APP_TEST.md) before its first device run. Its automatic sleep only dims the screen until the device sleep test passes.

The "Kaia Reader Rust" entry starts the Rust app on a Kobo Libra Colour. It does not replace the Lua app entry.

1. Eject the storage and unplug USB.
2. Choose "Kaia Reader Rust" in NickelMenu. On firmware 5.x, use a launcher that supports that firmware and point it at `/mnt/onboard/.adds/kaia/kaia-rust.sh`.
3. Wait for Home and the library scan. Books come from `/mnt/onboard`.
4. Open a book from Library. Use the page buttons or touch to turn pages.
5. Press both page buttons together to save and exit to Nickel. If one button arrives first, it may turn a page before the exit shortcut fires.
6. To use the Lua app again, choose "Kaia Reader" in NickelMenu.

Connecting a computer while the Rust app runs saves and exits to Nickel. Use Nickel's USB connection prompt to mount storage. The Rust app does not unmount storage while its database or books are open. A power-only charger updates charging state without asking the app to exit. This distinction still needs a device test.

The Rust launcher writes `/mnt/onboard/.kaia/rust.log` and keeps the last 500 KB at each launch. It restores the framebuffer depth and rotation on exit, then restarts Nickel if Nickel was running when it started. It counts failed starts for safe mode (see "Rust app safe mode" below). It does not use the Lua app's crash restart loop.

Rust settings, library data and reading state live in `/mnt/onboard/.kaia`. The Rust app reads packaged `fonts/`, `data/` and `resources/` beside its binary. It does not write the Lua app's settings, history or sidecars. Screen orientation starts in portrait. Sensor-driven rotation is not wired.

## Uninstall

1. Back up the Kobo first if you want to keep Kaia's data. `tools/kaia/device/backup.sh` copies `.adds/kaia` and the other items listed in `device/BACKUP_AND_RECOVERY.md`.
2. Connect the Kobo over USB. Delete `.adds/kaia` and `.adds/nm/kaia`.
3. If you added a KFMon entry for Kaia, delete `.adds/kfmon/config/kaia.ini` too.
4. Eject the Kobo, then unplug the cable.

The Rust data folder `.kaia` remains after uninstall. Back it up before deleting it.

Deleting `.adds/kaia` removes Kaia's settings, per-book reading data, sticker data and user patches. It does not remove books, and it does not touch stock KOReader's `.sdr` folders next to them.

Uninstalling does not remove the ExcludeSyncFolders line from `.kobo/Kobo/Kobo eReader.conf`. Remove that line by hand if you want Nickel to list hidden folders again.

## Where data lives

| Path | Contents |
|---|---|
| `.adds/kaia/` | Kaia program, launcher `kaia.sh`, `libs/libkaia.so`, and the settings file `settings.reader.lua` |
| `.adds/kaia/settings/`, `docsettings/`, `history/` | Kaia settings, document settings and reading history |
| `.adds/kaia/patches/` | User patches. Safe mode skips them. |
| `.adds/kaia/kaia/` | Sticker store |
| `.adds/kaia/kaia-startup-attempts` | Counter of failed starts for safe mode |
| `.adds/kaia/crash.log` | Launcher and app log. The launcher trims it to the last 500 KB at each launch. |
| `.adds/nm/kaia` | Lua and Rust NickelMenu entries |
| `.adds/kaia/kaia`, `kaia-rust.sh`, `resources/` | Rust binary, launcher and UI assets |
| `.kaia/settings.json`, `library.sqlite3`, `kaia/books/` | Rust settings, library index and reading state |
| `.kaia/rust.log` | Rust app and launcher log |
| `.kaia/safemode` | Empty file the user creates over USB to start the Rust app in safe mode |
| `.kaia/kaia-startup-attempts` | Failed-start count for Rust safe mode. The launcher and the app update it. |

Kaia keeps per-book data (reading position, highlights, notes) in its own folder, `.adds/kaia/docsettings/`. The first time Kaia opens a book that stock KOReader already read, it copies KOReader's `<book>.sdr` data, so highlights carry over. After that the two apps keep separate copies, and Kaia never changes or deletes KOReader's `.sdr` folder. You can switch this in Settings, Document, Book metadata location; choosing the book folder shares the `.sdr` folder with KOReader. Both apps also read fonts from `/mnt/onboard/fonts`.

## Safe mode

Safe mode turns off third-party plugins and user patches. Kaia shows a notice on the screen when it runs in safe mode.

- To start in safe mode, create an empty file named `safemode` in `.adds/kaia/` over USB, then start Kaia.
- To leave safe mode, delete that file, then start Kaia again.
- Kaia also starts in safe mode after three failed starts in a row. A healthy start clears the counter.

### Rust app safe mode

The Rust app has its own marker and counter in the Rust data folder. The Lua app's `safemode` file does not affect the Rust app, and the reverse holds too.

- To start the Rust app in safe mode, connect the Kobo over USB and create an empty file named `safemode` in the `.kaia` folder at the root of the storage, so the path is `/mnt/onboard/.kaia/safemode`. Eject the Kobo, then start "Kaia Reader Rust".
- To leave safe mode, delete that file over USB, then start the Rust app again. The Rust app does not delete the file itself.
- The launcher adds one to the count in `/mnt/onboard/.kaia/kaia-startup-attempts` before each start. A start whose count, after that increment, is above 3 runs in safe mode. So the fourth start after three failed starts is the first safe mode start.
- A start is healthy once its first screen has painted and stayed up for 10 seconds. A healthy start clears the count. A clean exit clears it too.
- A start in safe mode because of the count keeps the count until that start is healthy, as the Lua app does. A start that keeps failing stays in safe mode. A start in safe mode because of the marker keeps the marker.
- The Rust app shows a notice on the screen when it starts in safe mode. In safe mode the Rust app does not load user plugins. Bundled plugins still load.

## Crash handling

When Kaia exits with a crash, the launcher shows a crash screen and writes the last lines of `crash.log` to the screen. If Kaia crashes five times in a row, and each crash is less than 60 seconds after the previous one, the launcher stops restarting Kaia. It then restarts Nickel or reboots, as described above.

## Known gaps

- The 4.x package has run on one Libra Colour, with firmware 4.42.23303. No other Kobo model, other firmware version, or 5.x firmware has run it.
- Kaia has no update channel. The package does not update itself. Follow `docs/kaia/RELEASE_PROCESS.md` for upgrades. Kaia hides KOReader's update menu.
- The package has no Kaia artwork yet.
- The KFMon route for firmware 5.x is not verified.
- Kaia's own UI strings are in English only.
- The known problems of both apps, including the Rust app's sleep, dictionaries and page-turn timing, are in [release/NOTES.md](release/NOTES.md).

## For developers: build the package

The package builds, and `spec/kobo-package/test-package.sh` checks its layout and the launcher against the upstream script. The ARM smoke test runs the native library and LuaJIT under emulation. It runs by hand with `tools/kaia/rust-app-smoke.sh`, which needs Docker. CI does not run it.

```sh
source tools/kaia/env.sh
tools/kaia/build-rust-kobo.sh kobo build -p kaia-app --bin kaia --no-default-features --features kobo
tools/kaia/build-rust-kobo.sh kobov5 build -p kaia-app --bin kaia --no-default-features --features kobo
tools/kaia/package-kobo.sh dist/koreader-kobo-*.zip dist/kobo/libkaia.so dist/kaia
```

The script writes `dist/kaia/kaia-kobo-<version>-<hash>.zip` and `dist/kaia/SHA256SUMS`. The same input gives the same zip, byte for byte. Lua files come from the upstream zip. The packager adds `dist/rust/<target>/kaia`, the Rust launcher and UI resources from the checkout. CREngine uses the upstream `fonts/` and `data/` folders beside the binary. The packager runs `tools/kaia/check-rust-package.sh`; `bash spec/kobo-package/test-package.sh` also checks both packages and deterministic output.
