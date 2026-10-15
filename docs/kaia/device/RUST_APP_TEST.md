# First Rust app test on a Kobo Libra Colour

Status: procedure for the first device run. ARM emulation checks startup and
rendering only. It cannot check panel updates, touch calibration, power,
frontlight or USB handover.

## Prepare

1. Follow `BACKUP_AND_RECOVERY.md`. Back up the books, `.kobo`, `.adds/kaia`
   and `.kaia`. `backup.sh` copies `.kaia` when the folder exists, so check
   that `BACKUP_INFO.txt` lists it before continuing. Keep a copy off the
   device.
2. Record the firmware version and package SHA-256. Pick `kobo` for firmware
   4.x or `kobov5` for firmware 5.x. Do not put a 5.x binary on 4.x firmware.
3. Charge the battery. Copy a small EPUB with known text and a colour cover
   into a test folder under `/mnt/onboard`.
4. Install the package following `../INSTALL_KOBO.md`. Keep the Lua app's
   settings and the existing KOReader install. Do not remove either app's data.
5. Note Nickel's screen orientation and light level. Eject and unplug USB.
   For firmware 5.x, configure a supported launcher to run
   `/mnt/onboard/.adds/kaia/kaia-rust.sh`.

## Start and read

1. Start "Kaia Reader Rust". Home should appear upright and fill the panel.
   Wait for the library scan. Record the elapsed startup time.
2. Tap each navigation tab, then open Library. Confirm that touch follows
   the visible target, including near each corner. The initial orientation
   is portrait; automatic rotation is not connected.
3. Open the test EPUB. Check title, body text, margins, accents and cover
   colour. Compare the same page with the Lua app after exiting Rust.
4. Turn ten pages with the forward button and ten with the back button.
   Repeat with touch. Check missed inputs, duplicate turns and old pixels.
5. Exercise a two-finger gesture supported by the reader. Watch for a
   one-finger action firing partway through the gesture.
6. Press both page buttons together to exit. Nickel should return with its
   previous framebuffer depth and rotation. Start Rust again and reopen
   the book. Confirm that the last reading position survived.

## Light and power

Automatic sleep and plugin suspend requests currently show the sleep screen and
dim the light without kernel suspend on Kobo. `KOBO_AUTOMATIC_KERNEL_SUSPEND_ENABLED` in
`rust/kaia-app/src/app/power.rs` is `false`. Keep it false until this test passes
on the device, then change it to `true` and rebuild to test the timer. Manual
Sleep and the power button still request kernel suspend.

1. Disconnect USB and the charger. MTK cannot suspend safely while plugged in.
   Start Rust and check brightness, including zero, and warmth in Quick settings.
   Let the automatic sleep timer show its screen, then confirm a short Power press
   wakes it before testing manual Sleep.
2. Tap Sleep in Quick settings once. Wait at least 30 seconds. The sleep picture
   should finish drawing, the light should go off, and suspend should start after
   the 15-second screen delay and two-second subsystem delay.
3. Press Power briefly once. The book, touch, page buttons and previous light
   level should return. If suspend fails or returns immediately, Kaia should
   return to the awake screen on its own. Record that as a failed suspend test.
4. If it hangs, hold Power for about 15 seconds until the device restarts.
   Copy `.kaia/rust.log` over USB before launching Rust again. Do not keep retrying.
5. Check the log. Startup should list `kaia: input: power button /dev/input/event3`
   on this Libra Colour. The `kaia: power:` lines should show, in order:
   sleep requested, `schedule Suspend after 15s`, prepare, Wi-Fi scripts if needed,
   `state-extended` written to `1`, `schedule FinishSuspend after 2s`, panel
   completion, `run sync`, and the start of the `state` write with value `mem`.
   After Power, expect that write to end, `suspend returned`, resume cleanup,
   `state-extended` written to `0`, `Sleep(100ms)`, and `resume UI: full redraw
   requested`. Power-key lines may arrive before the worker reports completion.
   Each power diagnostic flushes and syncs the log file. Keep the final line if
   the sequence stops. A `FAILED` write should still lead to cleanup and redraw.
6. After one successful cycle, repeat five times, including a Power press in the
   two-second prepare window. Check that Kaia stays awake after cancellation.
   Check the Wi-Fi state before and after sleep. Do not enable sync or other
   network services for this test. Test a sleep cover if available.
7. Leave Kaia idle past the configured sleep timeout. Expect `automatic sleep:
   kernel suspend disabled; screen and light only`, with no prepare or `mem`
   write. Press Power to return. This temporary mode consumes more battery than
   kernel suspend, so return to Nickel when leaving the device unattended.
8. After manual sleep and wake pass, leave the device manually asleep for ten
   minutes and record battery readings before and after. This short test cannot
   establish overnight drain.

## USB and return to Nickel

1. With an EPUB open, connect a computer. The Rust app should save and exit.
   Nickel should offer its USB connection prompt. Accept it there.
2. Confirm that the storage mounts. Copy `.kaia/rust.log` to the computer.
   Check that `.kaia/settings.json`, `library.sqlite3` and `kaia/books/`
   exist. Compare the Lua settings and book sidecars with the backup.
3. Eject, unplug and start Rust again. Confirm the saved reading position.
4. Repeat while the Rust sleep screen is showing. Record whether USB wakes
   the device and whether Nickel's prompt appears.
5. Test a power-only charger. It should update charging state without
   returning to Nickel. Disconnect it and check the battery display again.
6. Exit with both page buttons together. Start "Kaia Reader" and confirm
   that the Lua app still opens its own settings and reading position.

## If something fails

1. Record the action, elapsed time, firmware, package checksum, book name and
   whether USB or a sleep cover was connected. Photograph the screen.
2. Try the two-page-button exit shortcut. If it works, copy `.kaia/rust.log`
   over USB before the next launch trims it.
3. If the app is stuck and a shell is already available, collect `dmesg`,
   the framebuffer `rotate` and `bits_per_pixel` values under
   `/sys/class/graphics/fb0/`, and `/proc/bus/input/devices`. Do not publish
   serial numbers, network credentials or book text without review.
4. From that shell, terminate the `kaia` process with TERM. The launcher
   should restore the framebuffer and restart Nickel. A forced termination
   can lose changes since the last saved state.
5. If no shell or exit shortcut works, use the device's normal forced restart
   procedure from the recovery guide. Keep the package and data for diagnosis.
6. Report the first failed step, expected and actual result, photo and log.
   For an EPUB failure, include a shareable sample and the saved position.
   For a suspend failure, include the time asleep and all wake attempts.

Do not make the Rust app the default until these checks pass on hardware.
