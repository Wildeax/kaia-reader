# Hardware profile (KR-002)

Status: the collector and this procedure exist. No profile from a real device is committed yet. The collector was tested against a fake root tree on a desktop only.

The profile is a sanitized, versioned record of one device and its firmware. Launcher and package choices follow this record, not assumptions about every Libra Colour unit.

## What the profile contains

`tools/kaia/device/collect-profile.sh` writes `key=value` lines to stdout.

| Key | Source on the device | Notes |
|---|---|---|
| `profile_format`, `collected_utc` | script | Format version 1 and UTC time. |
| `model_id` | `/bin/kobo_config.sh`, then the `PRODUCT` variable of udevd, then `/usr/bin/hwdetect.sh` | The upstream launcher uses the same order. The tagged KOReader backend names the Libra Colour as `KoboMonza`. Confirm the value on the device. |
| `platform` | `PLATFORM` variable of udevd | Selects the Wi-Fi driver path in KOReader. |
| `firmware_version` | field 3 of `/mnt/onboard/.kobo/version` | The upstream launcher reads the same field for its crash log. |
| `kernel_release`, `kernel_machine` | `uname -r`, `uname -m` | |
| `cpu_model`, `cpu_architecture`, `cpu_features` | `/proc/cpuinfo` | Only these three keys are read. The `Serial` line is never read. |
| `cpu_vfp`, `cpu_neon` | `cpu_features` | `yes`, `no` or `unknown`. |
| `dynamic_loader` | `/lib/ld-linux*` | Comma-separated list of loaders that exist. |
| `loader_float_abi_hint` | loader name | `ld-linux-armhf` suggests hard float. This is a hint. |
| `glibc_version` | release banner in `/lib/libc.so.6` | The loader is not executed. |
| `koreader_elf_float_abi` | `e_flags` of `luajit` in the Kaia or KOReader directory | `hard` or `soft`. This is the check that matters. |
| `mem_total_kb`, `mem_available_kb` | `/proc/meminfo` | |
| `fb_virtual_size`, `fb_bits_per_pixel`, `fb_rotate` | `/sys/class/graphics/fb0/` | |
| `fbink_*` | `fbink -e` from the KOReader or Kaia directory | Display facts such as `viewWidth`, `viewHeight`, `hasColorPanel`. |
| `storage_onboard_total_kb`, `storage_onboard_free_kb` | `df -P -k /mnt/onboard` | |
| `nickelmenu_present`, `kfmon_present` | directories `/mnt/onboard/.adds/nm` and `/mnt/onboard/.adds/kfmon` | Presence only. Versions are not read yet. |
| `koreader_dir_present`, `koreader_version` | `/mnt/onboard/.adds/koreader/git-rev` | |
| `kaia_dir_present`, `kaia_version` | `/mnt/onboard/.adds/kaia/git-rev` | |
| `profile_file_name` | `model_id` and `firmware_version` | Suggested name for the committed file. |

The profile never contains the device serial, MAC addresses, Wi-Fi network names, account e-mail addresses or database contents. The collector does not open `.kobo/KoboReader.sqlite`, the Wi-Fi configuration or any network interface file. As a second layer, the script removes any value that contains the serial, and it drops lines with an e-mail-like value, a MAC-like value or the word "ssid".

Fields 2 and 4 and later of the version file are not decoded. The collector does not print them. Add them only after someone reviews a sanitized sample.

Example output from the test fixture (fake values, not a device):

```
profile_format=1
model_id=monza
firmware_version=4.38.23171
dynamic_loader=/lib/ld-linux-armhf.so.3
loader_float_abi_hint=hard
glibc_version=2.23
koreader_elf_float_abi=hard
fb_virtual_size=1264,1680
fb_bits_per_pixel=32
profile_file_name=monza-4.38.23171.txt
```

## How to run it on the device

The collector needs a POSIX shell and BusyBox tools. It writes no files on the device. It uses only read access, plus `fbink -e`, which draws nothing.

1. Open a shell on the device. Use the SSH plugin that ships with KOReader, or any shell you already use for development. Do not store device passwords in the repository.
2. From the computer, stream the script to the device shell and keep the output locally:

   ```sh
   ssh <user>@<device-address> 'sh -s' < tools/kaia/device/collect-profile.sh > profile.txt
   ```

   The script is read from standard input, so nothing is copied onto the device.
3. Check the file before it goes anywhere. Read the serial into a local variable, then count matches. The serial does not appear in the terminal or in a log:

   ```sh
   ssh <user>@<device-address> 'cut -d, -f1 /mnt/onboard/.kobo/version' | tr -d '\n' > /tmp/kaia-serial.txt
   grep -c -F -f /tmp/kaia-serial.txt profile.txt    # must print 0
   grep -c -E '@|ssid|([0-9A-Fa-f]{2}:){5}' profile.txt    # must print 0
   rm -f /tmp/kaia-serial.txt
   ```

4. Check that every line is `key=value`, then move the file into `docs/kaia/device/profiles/` under the name in `profile_file_name`.

To test the collector on a desktop, run `sh tools/kaia/device/test-collect-profile.sh`. It builds a fake root with a fake serial, Wi-Fi name, MAC address, e-mail address and database marker. It checks that none of them reach the output.

## Where to commit results

Commit one file per device model and firmware pair:

```
docs/kaia/device/profiles/<model>-<firmware>.txt
```

For example, `profiles/monza-4.38.23171.txt` would hold the profile for a Libra Colour (if `model_id` is `monza`) on that firmware. Use the `profile_file_name` line to pick the name. Do not overwrite a profile for an older firmware. Keep one file per firmware, so the history of tested firmware stays visible.

Commit the profile only after the check in step 3 passes.

## Launcher and package selection

The profile's `firmware_version` decides the launcher and package. Do not infer firmware from the model, the purchase date or a neighboring device.

1. **Package.** Pick the KOReader release package that matches the firmware generation: `kobo` or `kobov5`. The plan records that KOReader publishes separate `kobo` and `kobov5` packages and toolchain targets (ENGINEERING_PLAN.md, section 1, source S10 and S11). This repository does not hold the exact firmware range for each package. Check the release assets for the baseline tag before choosing, and record the choice in the profile file name or in the release notes.
2. **Launcher.** Use the launcher script from the same package. Do not copy `platform/kobo/koreader.sh` from this tree onto a device whose firmware or package differs from the one it was tested with. Any Kaia change to the launcher needs a test on each firmware it ships with.
3. **NickelMenu.** The plan records that NickelMenu says firmware 5.x is unsupported (source S9). For a profile with firmware 5.x, do not install NickelMenu and do not count it as a recovery path. Use it on 4.x only after a test on that firmware passes and the result is recorded.
4. **KFMon.** The profile records only that its directory exists. Its supported firmware range is not recorded in this repository. Check it the same way as NickelMenu before relying on it.
5. **Firmware updates.** After every Kobo firmware update, collect a new profile. Treat the new firmware as untested until the launcher, the package and the recovery tests in BACKUP_AND_RECOVERY.md pass on it. Do not roll back firmware as a general fix. The plan says blind downgrades and indefinite refusal of security updates are not a strategy.
6. **Support claim.** Kaia claims support only for the firmware whose profile and test results are committed. The plan limits the first release to one certified combination of hardware and firmware.

The upstream launcher chooses its Wi-Fi driver path from `PLATFORM`, reads the same firmware field for its crash log, and uses `PRODUCT` and the `fbink -e` output to choose display settings. A profile whose values differ from what the launcher expects is a reason to stop and review the launcher before installing.

## Open items

- Confirm on a device: the `model_id` value for the Libra Colour, the `git-rev` file in the Kaia and KOReader directories, `df -P -k` output under BusyBox, the glibc banner string in `libc.so.6`, and the `PLATFORM` value from udevd.
- Decide the firmware range for `kobo` and `kobov5` packages. This needs the release assets, not guesses.
- Decide whether NickelMenu and KFMon versions should be read from their files.
