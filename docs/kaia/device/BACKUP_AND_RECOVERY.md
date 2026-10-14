# Backup and recovery (KR-003)

Status: the backup and restore scripts exist. `tools/kaia/device/test-backup-restore.sh` checks both against a fake volume on a desktop, including `.kaia`. The Kaia safe mode described below is a design requirement. It is not implemented in this tree yet, so the KR-003 acceptance test cannot pass until it is.

Owner: platform engineer and QA. Depends on the hardware profile (HARDWARE_PROFILE.md).

## What the backup covers

`tools/kaia/device/backup.sh` copies these items from a Kobo mounted over USB:

| Item | Location on the device | Notes |
|---|---|---|
| Kaia application | `.adds/kaia/` | Whole directory, including the plugin tree and the Lua app's state files. |
| Kaia data (Rust app) | `.kaia/` | Whole directory: `settings.json`, reading state in `kaia/books/*.json`, `kaia/kaia.sqlite3`, `library.sqlite3`, `statistics.sqlite3`, `fonts/`, `dict/` and `cache/`. The cache is not skipped, so it is backed up too and counts toward the backup size. Copied only if the folder exists. |
| KOReader settings and user patches | `.adds/koreader/settings.reader.lua`, `settings`, `patches`, `defaults.persistent.lua`, `defaults.custom.lua` | Each item is copied only if it exists. Check the list on the device once. |
| NickelMenu configuration | `.adds/nm/` | |
| Book sidecars | every `*.sdr` directory outside `.kobo` and `.adds` | KOReader keeps highlights, notes and reading position here. |
| Nickel database (optional) | `.kobo/KoboReader.sqlite` | Only with `--include-nickel-db`. Copied as a read-only file. |

The backup does not copy book files. Copy the library folders separately if they belong in the backup set.

The Rust app keeps its data in `.kaia`, a folder beside `.adds`. The backup copies `.kaia` with the same hashes and manifest as the other items. The script skips nothing inside it.

To run the fake-volume check on a desktop, with no device:

```sh
sh tools/kaia/device/test-backup-restore.sh
```

The backup mirrors device paths. A file at `.adds/kaia/core.lua` on the device is at `.adds/kaia/core.lua` in the backup. Every file has a SHA-256 hash in `MANIFEST.sha256`. `BACKUP_INFO.txt` records the time, the mount name, whether the database was included and the list of copied items. It does not record the computer's own paths.

The backup script never writes to the Kobo volume. It refuses a backup directory inside the volume.

## Before a risky change

1. Collect the hardware profile if the firmware changed since the last one.
2. Connect the Kobo over USB and choose the connect option in Nickel. Wait until the volume mounts on the computer. On macOS the volume is usually `/Volumes/KOBOeReader`. Check the name in Finder.
3. Run the backup into a folder outside the volume:

   ```sh
   sh tools/kaia/device/backup.sh /Volumes/KOBOeReader ~/kaia-backups
   ```

   Add `--include-nickel-db` only when the device is in USB mass-storage mode. That mode means Nickel is not writing the database. The script stops if the file changes while it copies.

4. Check the manifest inside the backup directory:

   ```sh
   cd ~/kaia-backups/kaia-backup-<time>
   shasum -a 256 -c MANIFEST.sha256        # on Linux: sha256sum -c
   ```

5. Eject the volume in Finder. Then unplug the cable.

## Restoring the Kaia directory

`tools/kaia/device/restore.sh` restores `.adds/kaia` and `.kaia`. It restores `.kaia` only when the backup contains it, so a backup made before `.kaia` was added leaves the current data folder alone. The two folders restore together. If either copy fails, both return to their state before the run. It runs as a dry run unless `--apply` is given.

1. Connect the Kobo over USB and wait for the volume to mount. Kaia must not be running. A running app writes to storage.
2. Dry run:

   ```sh
   sh tools/kaia/device/restore.sh ~/kaia-backups/kaia-backup-<time> /Volumes/KOBOeReader
   ```

   The dry run checks every backup file against the manifest and prints the plan. It writes nothing.
3. Apply:

   ```sh
   sh tools/kaia/device/restore.sh ~/kaia-backups/kaia-backup-<time> /Volumes/KOBOeReader --apply
   ```

   The script renames the current `.adds/kaia` to `.adds/kaia.replaced-<UTC time>` and the current `.kaia` to `.kaia.replaced-<UTC time>`. It copies both from the backup and checks every restored file. If a copy fails or a file does not match the manifest, it removes the new copies and moves both replaced directories back.
4. Eject the volume. Start Kaia from its launch entry. Delete `.adds/kaia.replaced-*` and `.kaia.replaced-*` by hand after Kaia starts from the restored copy. Keep them until then: they hold the state from before the restore.

Settings in `.adds/koreader` and `.adds/nm` are not restored by this script. Copy them by hand from the backup if needed, using the same USB procedure.

### What the restore never does

- It never deletes or changes a book file, a sidecar, `.kobo` or `.adds/koreader`.
- It never restores `.kobo/KoboReader.sqlite`. Nickel's database is not part of Kaia's recovery. Do not copy the database over the live file.
- It never runs on a backup whose manifest does not match. The check runs before any write.
- It never changes `.kaia` when the backup has no `.kaia`.
- The only deletion it performs is the rollback of its own new copies of `.adds/kaia` and `.kaia`.

### Factory reset is not a recovery path

A factory reset removes books and reading data. It fails the KR-003 acceptance criterion. Do not use it to recover from a Kaia failure.

## Recovery paths

| Path | When to use | What it needs | Limits |
|---|---|---|---|
| **1. Kaia safe mode (automatic)** | A third-party plugin or a user patch breaks startup. | Safe mode in the Lua launcher (`kaia.sh`) and the Rust launcher (`kaia-rust.sh`). The launcher logic has host tests; no device test yet. | Does not help when the launcher itself is broken. |
| **2. Return to Nickel** | Kaia fails before its UI starts, or the launch entry is the problem. | Nothing extra. Nickel and its launchers stay installed. | If the launch entry runs a broken launcher, the screen may stay blank. Power the device off and on again. |
| **3. USB manual restore** | The launcher, the binary or the app directory is broken and safe mode cannot start. | The backup, the scripts and a USB connection that mounts the volume. | Needs the volume to mount while Kaia is in its failure state. Test this (see test B). |
| **4. Remove the launch entry** | The entry itself keeps starting Kaia. | Access to the NickelMenu or KFMon configuration over USB. | Only for entries you created. Leave Nickel's own entries alone. |

### 1. Kaia safe mode

Required behavior, for the Kaia launcher owner to implement:

- Safe mode starts when either condition holds:
  - three startup failures happened in a row, or
  - the file `.adds/kaia/safemode` exists at launch.
- A startup failure is counted in a file that survives the launcher's own restarts. The upstream launcher keeps its crash count in shell variables and resets it after 20 seconds without a crash, so it cannot be reused for this. Kaia writes the counter at the start of each launch and clears it after a healthy start, for example when the first screen is drawn and stays up for a fixed time. The exact values belong to the launcher owner and need a test.
- In safe mode, Kaia does not load third-party plugins or user patches. Core Kaia starts, so the user can open a book, read it and leave.
- Safe mode shows a visible notice on screen. Leaving safe mode removes the `safemode` file and the failure count.

Rust app, as built:

- The marker is `/mnt/onboard/.kaia/safemode`, in the Rust data folder. The counter is `/mnt/onboard/.kaia/kaia-startup-attempts`. The Lua app's files in `.adds/kaia` are separate.
- `platform/kobo/kaia/kaia-rust.sh` adds one to the counter before each launch. A count above 3 after that increment, or the marker, starts the app with `--safe-mode`. A clean exit (status 0) removes the counter.
- After a healthy start, the app removes the counter. A healthy start is a first screen that has painted and stayed up for 10 seconds (`rust/kaia-app/src/safe_mode.rs`, `HEALTHY_AFTER`). A start in safe mode because of the count keeps the counter until that start is healthy, so a start that keeps failing stays in safe mode. The marker stays until the user deletes it.
- In safe mode the app skips user plugins and shows a notice that says how to leave safe mode.
- Tests: `sh platform/kobo/kaia/test-kaia-rust.sh` runs the real launcher with a fake binary (marker, count below and at the limit, clean exit, repeated crashes). `cargo test -p kaia-app --test safe_mode` covers the app side.

The upstream launcher is in `platform/kobo/koreader.sh`. Its restart loop stops after five consecutive crashes and keeps the crash log in `crash.log`. Safe mode must work with that loop, not replace it silently.

### 2. Return to Nickel

Nickel stays on the device. Kaia starts only from a launch entry, so a reboot without the entry gives the stock reader again. To reboot, power the device off with the power button and then power it on.

The upstream launcher stops Nickel before it starts the reader. If the launcher fails at that point, the screen can stay blank. Reboot, then go to path 3.

### 3. USB manual restore

Use this path when safe mode cannot run. Steps are in the restore section above. The Kobo must present its storage over USB. Test whether it does while Kaia is in a crash loop. Record the result. If the volume does not mount in that state, the manual path does not cover that failure, and the gap goes in the KR-003 report.

### 4. Remove the launch entry

If the entry keeps starting a broken app, move the Kaia entry out of the launcher configuration over USB. Keep Nickel's own entries and KOReader's entries unchanged.

## Failures that need manual recovery

Automatic safe mode does not cover these. Do not promise automatic repair for them.

- **Broken launcher or launch script.** Safe mode runs inside the launcher, so a broken launcher cannot start it. Use path 3.
- **Failure before the counter is written.** A failure in the first lines of the launcher, or while reading a broken settings file, can happen before safe mode checks anything. Use path 2 or 3.
- **Interrupted or partial Kaia update.** A partly written update leaves mixed files. Restore `.adds/kaia` from a backup (path 3). Keep the backup from before the update.
- **Full storage or a damaged FAT volume.** Eject the volume, then check it with the computer's disk tools. Do not repair a volume while it is mounted by the Kobo or by Finder.
- **Firmware update that removes or changes a launcher.** The Kaia entry may disappear or stop working. Collect a new profile, then reinstall the launcher for that firmware by hand. Test before use.
- **Nickel itself does not start.** This is outside Kaia's recovery. Follow Kobo's own support procedure. Do not touch Nickel's database.
- **Lost data after the last backup.** Kaia data written after the last backup is lost when the backup is restored. Back up before each risky change.

## Injected startup failure test (KR-003 acceptance)

The acceptance test has two parts. Test A checks automatic recovery through safe mode. Test B checks the manual USB path. Plan section 10 requires recovery to work from outside the failed UI, so both parts are needed before KR-003 can be signed off. Run each test three times on the same firmware and record each result.

### Preconditions

- The device has a committed hardware profile for the firmware under test.
- A candidate Kaia build is installed in `.adds/kaia`. For test A, the build includes the safe mode described above.
- A backup made with `backup.sh` exists, and its manifest checks out.
- Note the SHA-256 of two or three book files and their sidecars. Use the manifest, or run `shasum -a 256` on the mounted volume. Keep the list with the test log.
- Use a test copy of Kaia data where possible. The plan says to clone data rather than share a live writable profile.

### Test A: injected plugin fault, automatic recovery

1. Back up the device as described in "Before a risky change". Check the manifest. Eject and unplug.
2. Mount the volume again. Create a test plugin in `.adds/kaia/plugins/zz-injected-fault.koplugin/`. Copy the layout of KOReader's example plugin, `plugins/hello.koplugin` (source S3 in the plan), so the loader accepts it. Its `main.lua` contains one line:

   ```lua
   error("injected startup fault for KR-003")
   ```

3. Eject, unplug and wait for Nickel.
4. Launch Kaia from the usual entry. Record each start:
   - the time from the tap to the crash screen,
   - the crash number on screen,
   - the `crash.log` lines written for each failure.
5. Expect a crash screen on the first and second failed starts. On the third consecutive failure, Kaia starts in safe mode. The safe-mode notice must be visible.
6. In safe mode, open one of the test books. Check that the page and the highlights match what they were before the test. Turn a few pages.
7. Connect over USB. Check the book and sidecar hashes against the list from the preconditions. Check that the book count is unchanged.
8. Recover: remove `.adds/kaia/plugins/zz-injected-fault.koplugin/`. Leave safe mode from the notice or by removing the `safemode` file. Launch Kaia. Expect a normal start with the plugin missing.

**Pass:** safe mode starts after three failures, the book opens in safe mode, the hashes match, no book is deleted and the device is not factory reset. A normal start works after the fault is removed and the failure counter is clear.

### Test B: injected launcher fault, manual USB restore

1. Back up the device. Check the manifest. Eject and unplug.
2. Mount the volume again. Replace the Kaia launch script (the file the launch entry runs) with a script that exits with status 1 at once. Keep the original in a safe place on the computer. Eject and unplug.
3. Launch Kaia from the usual entry. Expect no working UI. Record what the screen shows.
4. Reboot the device: power it off, then power it on. Expect Nickel.
5. Connect over USB. Record whether the volume mounts. If it does not mount, stop. Record the failure, and the KR-003 report must say this path is not available for this failure.
6. If the volume mounts, run the restore dry run, then `--apply`, against the backup from step 1. Eject. Reboot. Launch Kaia. Expect a normal start.
7. Check the book hashes against the list from the preconditions.

**Pass:** the device returns to Nickel, the volume mounts over USB, the restore completes, Kaia starts normally, and no book is deleted or changed.

### Record for each run

- firmware and profile file name,
- Kaia build identifier,
- backup directory name and its manifest result,
- each crash number and the time between starts,
- the result of the hash check,
- any step that did not match the expected result.

Store the record with the test notes, not in public logs. Book names and hashes are not secrets, but the serial and account data must not appear.
