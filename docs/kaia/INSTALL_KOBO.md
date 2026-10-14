# Install Kaia Reader on a Kobo

Status: not tested on a device. The package builds, and `spec/kobo-package/test-package.sh` checks its layout and the launcher against the upstream script. The ARM smoke test runs the native library and LuaJIT under emulation. Neither check replaces a test on a Kobo. Follow the device test procedure in `docs/kaia/device/BACKUP_AND_RECOVERY.md` before using Kaia for a library you care about.

Kaia installs next to stock KOReader. It has its own folder, its own settings file and its own launch entry. It does not download KOReader packages, and it hides KOReader's update menu.

## Choose the package

Kaia builds one package per firmware family.

| Firmware | Package file name | Built from |
|---|---|---|
| 4.x | `kaia-kobo-<version>-<hash>.zip` | `dist/koreader-kobo-*.zip` and `dist/kobo/libkaia.so` |
| 5.x | `kaia-kobo-v5-<version>-<hash>.zip` | `dist/koreader-kobov5-*.zip` and `dist/kobov5/libkaia.so` |

Both packages include the Lua app and an optional Rust app. Choose the package that matches your firmware. Neither package has passed a real device test.

## Check the firmware version

There are two ways to read the firmware version.

1. On the Kobo, open the settings and find the device information page. It shows the firmware version. Menu names differ between firmware versions.
2. Over USB, open the file `.kobo/version` on the Kobo storage. The third comma-separated field is the firmware version. An example value is `4.38.23171`.

## Install

1. Connect the Kobo to the computer with USB. Wait until the storage mounts.
2. Unzip the package at the root of the Kobo storage. The zip contains `.adds/kaia/` and `.adds/nm/kaia`. Let the file manager merge the `.adds` folder with the existing one. Do not delete `.adds/koreader`.
3. Eject the Kobo in the file manager, then unplug the cable.
4. Start Kaia from the NickelMenu entry "Kaia Reader".

NickelMenu is not part of the package. Install it separately and follow its own documentation. The entry file `.adds/nm/kaia` has separate Lua and Rust entries. This repository does not include NickelMenu's documentation, so the entry syntax was written from the documented format and not checked against the NickelMenu version on the device. If the entry does nothing, check its syntax against that version.

NickelMenu states that it does not support firmware 5.x. On firmware 5.x, start Kaia with another launcher, such as KFMon, and follow that launcher's documentation. The Kaia launcher reads `.adds/kfmon/config/kaia.ini` when it exits under KFMon. If that file contains the line `reboot_on_exit=false`, the launcher restarts Nickel. Without that line, the launcher reboots the device.

## Start the Rust app

The Lua app remains the default entry, "Kaia Reader". The separate
"Kaia Reader Rust" entry starts the Rust app on a Kobo Libra Colour.
Read `docs/kaia/device/RUST_APP_TEST.md` before its first device run.

1. Eject the storage and unplug USB.
2. Choose "Kaia Reader Rust" in NickelMenu. On firmware 5.x, use a launcher
   that supports that firmware and point it at
   `/mnt/onboard/.adds/kaia/kaia-rust.sh`.
3. Wait for Home and the library scan. Books come from `/mnt/onboard`.
4. Open a book from Library. Use the page buttons or touch to turn pages.
5. Press both page buttons together to save and exit to Nickel. If one button
   arrives first, it may turn a page before the exit shortcut fires.
6. Choose "Kaia Reader" in NickelMenu to return to the Lua app.

Connecting a computer while the Rust app runs saves and exits to Nickel.
Use Nickel's USB connection prompt to mount storage. The Rust app does not
unmount storage while its database or books are open. A power-only charger
updates charging state without asking the app to exit. This distinction
still needs a device test.

The Rust launcher writes `/mnt/onboard/.kaia/rust.log` and keeps the last
500 KB at each launch. It restores the framebuffer depth and rotation on
exit, then restarts Nickel if Nickel was running when it started. It counts
failed starts for safe mode (see "Safe mode" below). It does not use the Lua
app's crash restart loop.

Rust settings, library data and reading state live in `/mnt/onboard/.kaia`.
The Rust app reads packaged `fonts/`, `data/` and `resources/` beside its
binary. It does not write the Lua app's settings, history or sidecars.
Screen orientation starts in portrait. Sensor-driven rotation is not wired.

## Uninstall

1. Back up the Kobo first if you want to keep Kaia's data. `tools/kaia/device/backup.sh` copies `.adds/kaia` and the other items listed in `BACKUP_AND_RECOVERY.md`.
2. Connect the Kobo over USB. Delete `.adds/kaia` and `.adds/nm/kaia`.
3. If you added a KFMon entry for Kaia, delete `.adds/kfmon/config/kaia.ini` too.
4. Eject the Kobo, then unplug the cable.

The Rust data folder `.kaia` remains after uninstall. Back it up before deleting it.

Deleting `.adds/kaia` removes Kaia's settings, per-book reading data, sticker data and user patches. It does not remove books, and it does not touch stock KOReader's `.sdr` folders next to them.

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

The Rust app has its own marker and counter in the Rust data folder. The Lua
app's `safemode` file does not affect the Rust app, and the reverse holds too.

- To start the Rust app in safe mode, connect the Kobo over USB and create an
  empty file named `safemode` in the `.kaia` folder at the root of the storage,
  so the path is `/mnt/onboard/.kaia/safemode`. Eject the Kobo, then start
  "Kaia Reader Rust".
- To leave safe mode, delete that file over USB, then start the Rust app again.
  The Rust app does not delete the file itself.
- The launcher adds one to the count in `/mnt/onboard/.kaia/kaia-startup-attempts`
  before each start. A start whose count, after that increment, is above 3 runs
  in safe mode. So the fourth start after three failed starts is the first safe
  mode start.
- A start is healthy once its first screen has painted and stayed up for 10
  seconds. A healthy start clears the count. A clean exit clears it too.
- A start in safe mode because of the count keeps the count until that start is
  healthy, as the Lua app does. A start that keeps failing stays in safe mode. A
  start in safe mode because of the marker keeps the marker.
- The Rust app shows a notice on the screen when it starts in safe mode. In
  safe mode the Rust app does not load user plugins. Bundled plugins still load.

## Crash handling

When Kaia exits with a crash, the launcher shows a crash screen and writes the last lines of `crash.log` to the screen. If Kaia crashes five times in a row, and each crash is less than 60 seconds after the previous one, the launcher stops restarting Kaia. It then restarts Nickel or reboots, as described above.

## Build the package

```sh
source tools/kaia/env.sh
tools/kaia/build-rust-kobo.sh kobo build -p kaia-app --bin kaia --no-default-features --features kobo
tools/kaia/build-rust-kobo.sh kobov5 build -p kaia-app --bin kaia --no-default-features --features kobo
tools/kaia/package-kobo.sh dist/koreader-kobo-*.zip dist/kobo/libkaia.so dist/kaia
```

The script writes `dist/kaia/kaia-kobo-<version>-<hash>.zip` and `dist/kaia/SHA256SUMS`. The same input gives the same zip, byte for byte. Lua files come from the upstream zip. The packager adds `dist/rust/<target>/kaia`, the Rust launcher and UI resources from the checkout. CREngine uses the upstream `fonts/` and `data/` folders beside the binary. The packager runs `tools/kaia/check-rust-package.sh`; `bash spec/kobo-package/test-package.sh` also checks both packages and deterministic output.

## Known gaps

- The package has not been tested on a Kobo, for either firmware family.
- Kaia has no update channel. The package does not update itself. Follow `docs/kaia/RELEASE_PROCESS.md` for upgrades. Kaia hides KOReader's update menu.
- The package has no Kaia artwork yet. A later task adds it.
- The NickelMenu entry syntax is not checked against NickelMenu on the device (see above).
