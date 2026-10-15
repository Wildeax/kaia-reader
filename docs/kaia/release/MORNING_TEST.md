# Morning test: Kaia Reader 0.1.0 on the Kobo Libra Colour

Run this after the overnight build is installed. Work from top to bottom. The steps start with the safest and end with the riskiest. The exit in step 8 comes after the sleep test because the device must be awake to exit. Steps 9 and 10 check the Lua app and collect the results.

If the device stops responding, follow the recovery steps in that step, then stop the test there.

## Before you start

- Write down the firmware version. The install guide explains how to read it.
- Write down the name of the package you installed. Its name contains the build hash.
- Back up the Kobo. Include `.kobo`, `.adds/kaia`, `.kaia` and the books. The backup guide is [device/BACKUP_AND_RECOVERY.md](../device/BACKUP_AND_RECOVERY.md). Check that `BACKUP_INFO.txt` lists `.kaia`.
- Charge the battery.
- Copy a small EPUB with known text and a colour cover into a test folder under `/mnt/onboard`. Copy a small PDF into the same folder for step 6.
- Note Nickel's screen orientation and light level.
- Have the same EPUB ready in Nickel for step 2.
- Eject the Kobo and unplug USB before you start step 1.

## The steps

### 1. Open Kaia Reader Rust from NickelMenu

Do:

- Check that NickelMenu lists "Kaia Reader" and "Kaia Reader Rust".
- Tap "Kaia Reader Rust". Count the seconds until Home appears, and write the number down.

Expect:

- Home appears upright and fills the screen. On 2026-10-14 the first start took about 20 seconds.
- The library scan finishes, and your books appear.
- No safe mode notice appears. If a notice appears, write it down and go on.

If it goes wrong:

- If Home does not appear within two minutes, press both page buttons together.
- If nothing happens, hold Power for about 15 seconds until the device restarts.
- Copy `.kaia/rust.log` over USB before you start Rust again. See the last section.
- Do not keep retrying.

### 2. Compare the reading page with Nickel

Do:

- Open the test EPUB in Kaia Reader Rust from Library.
- Open the same book in Nickel at the same passage.
- Compare one full page in each app. Compare the number of body lines, the text size, the margins, the accents, and the cover colour if the page has a cover.

Expect:

- On the reference page with Nickel's large default, the text has nine body lines. Kaia shows the same number of lines on the same page.
- A header and a footer appear, as they do in Nickel.
- The text size and margins look close to Nickel's.
- Accents and the cover colour look correct.

If it goes wrong:

- Write down the number of body lines in each app.
- Take a photo of both screens.
- Do not change the text size or margins to fix it during this test.

### 3. Turn pages with the buttons and by tap

Do:

- Press the forward page button ten times. Then press the back page button ten times.
- Tap the right side of the page ten times. Then tap the left third of the page ten times.
- Keep taps out of the top tenth of the screen and out of the top right corner. A tap in the top tenth opens the reading menu. A tap in the top right corner adds a bookmark. Tap it again to remove the bookmark.

Expect:

- Each press or tap turns one page.
- Forward and back move through the same pages in the same order.
- No page is skipped or repeated, and no old text stays on the screen.

If it goes wrong:

- Write down the press or tap number, the page before, and the page after.
- Note whether the screen showed old text or a blank page.
- Take a photo.

### 4. Reading menu, font panel and light panel

Do:

- Tap the centre of the page. The reading menu opens.
- Tap "Text and layout". Look at the font panel. Close it.
- Tap "Light". Move the Frontlight slider up and down once, then set it back.
- Do not tap Sleep in this step. Step 7 covers it.

Expect:

- The reading menu shows Home, Contents, Notes, Text and layout, Light and More.
- "Text and layout" opens the font panel with Font, Size, Weight, Margins and Hyphenation.
- "Light" opens Quick settings with Frontlight, Warmth, Wi-Fi, Airplane mode, Lock orientation, Orientation, Toggle light and Sleep.
- The screen gets brighter and darker as the Frontlight slider moves.

If it goes wrong:

- Write down the control you tapped and what appeared instead.
- Take a photo.

### 5. Home, Library, More and Settings

Do:

- Close Quick settings. Open the reading menu and tap Home.
- Tap a book on Home. Check its cover and title, then go back.
- Tap Library. Nickel calls this screen My books. Tap near each corner of the screen and near the bottom edge, where the buttons are.
- Open a book, tap the centre of the page, and tap More. Look at the sheet, then close it.
- Tap Home, then tap Settings. Open the first section, then go back.

Expect:

- Each screen opens when you tap its button, and nothing is drawn over it.
- Library shows the book covers, with filter and sort controls.
- More opens a sheet with items such as Go to the end, Mark as finished, Header and footer, Word list and Plugins.
- Settings shows its list of sections.
- Touch follows the button you see, including near the corners.

If it goes wrong:

- Write down the screen name and the control.
- Take a photo.
- Tap Home to get back to a known screen.

### 6. PDF zoom

Do:

- Open the test PDF from Library.
- Pinch outward with two fingers on a page. Hold the zoom for a moment.
- Drag with one finger to move around the zoomed page.
- Pinch inward until the page fits again.

Expect:

- The page zooms about three times around your fingers. The text is sharp when you lift your fingers.
- Dragging moves the page.
- Pinching inward returns to the fitted page.
- A tap while zoomed may not turn to the next page the way Nickel does. Nickel's tap-to-turn at the same zoom and its page map are not in Kaia yet. This is a known difference. Write down what happens.

If it goes wrong:

- Write down the zoom level and the gesture that failed.
- Take a photo.

### 7. Sleep test (riskiest)

This step copies the sleep test in [device/RUST_APP_TEST.md](../device/RUST_APP_TEST.md), section "Light and power". Run 7a to 7e in order. Run it only after steps 1 to 6 are done or noted. Do not run the rest of that section today. Its repeats, its idle sleep test and its ten-minute rest are for a later session.

Write down the time when you tap Sleep and the time when you press Power.

#### 7a. Check the light, then wake the automatic sleep screen

Do, as written in RUST_APP_TEST.md:

> Disconnect USB and the charger. MTK cannot suspend safely while plugged in. Start Rust and check brightness, including zero, and warmth in Quick settings. Let the automatic sleep timer show its screen, then confirm a short Power press wakes it before testing manual Sleep.

Expect:

- Brightness changes down to zero and back up. Warmth changes too.
- The automatic sleep screen appears.
- A short Power press returns you to the book.

If it goes wrong:

- If the screen does not come back after a short Power press, go to 7d.

#### 7b. Tap Sleep once

Do, as written in RUST_APP_TEST.md:

> Tap Sleep in Quick settings once. Wait at least 30 seconds. The sleep picture should finish drawing, the light should go off, and suspend should start after the 15-second screen delay and two-second subsystem delay.

Do not tap anything else during the wait.

Expect:

- The sleep picture finishes drawing.
- The light goes off.

If it goes wrong:

- If the sleep picture does not finish drawing, wait until the 30 seconds have passed, then go to 7c.

#### 7c. Press Power once

Do, as written in RUST_APP_TEST.md:

> Press Power briefly once. The book, touch, page buttons and previous light level should return. If suspend fails or returns immediately, Kaia should return to the awake screen on its own. Record that as a failed suspend test.

Expect:

- The book, touch, the page buttons and the previous light level return.

If it goes wrong:

- If the screen returns on its own after a failed suspend, write down "failed suspend test" with the times you noted.
- If the device does not respond, go to 7d.

#### 7d. If it hangs

Do, as written in RUST_APP_TEST.md:

> If it hangs, hold Power for about 15 seconds until the device restarts. Copy `.kaia/rust.log` over USB before launching Rust again. Do not keep retrying.

Expect:

- The device restarts into Nickel after about 15 seconds.

After the restart, copy `.kaia/rust.log` over USB. Then stop the sleep test.

#### 7e. Check the log

Do, as written in RUST_APP_TEST.md. The developer reads this part from the copied log, so you do not need to read it on the device:

> Check the log. Startup should list `kaia: input: power button /dev/input/event3` on this Libra Colour. The `kaia: power:` lines should show, in order: sleep requested, `schedule Suspend after 15s`, prepare, Wi-Fi scripts if needed, `state-extended` written to `1`, `schedule FinishSuspend after 2s`, panel completion, `run sync`, and the start of the `state` write with value `mem`. After Power, expect that write to end, `suspend returned`, resume cleanup, `state-extended` written to `0`, `Sleep(100ms)`, and `resume UI: full redraw requested`. Power-key lines may arrive before the worker reports completion. Each power diagnostic flushes and syncs the log file. Keep the final line if the sequence stops. A `FAILED` write should still lead to cleanup and redraw.

Copy `.kaia/rust.log` over USB whatever the result. See the last section.

### 8. Exit with both page buttons

Do:

- With the device awake, open the test EPUB in Kaia Reader Rust.
- Press both page buttons together.
- Start Kaia Reader Rust again from NickelMenu. Open the same book.

Expect:

- Kaia saves, and Nickel returns with its usual screen orientation and depth.
- After you start Rust again, the book opens at the reading position you left.

Note: if one button arrives first, it may turn a page before the exit fires. That is expected.

If it goes wrong:

- Try the two buttons once more.
- If Kaia still does not exit, hold Power for about 15 seconds, then copy `.kaia/rust.log` over USB.

### 9. Lua app quick check

Do:

- In NickelMenu, tap "Kaia Reader". This is the Lua app.
- Open the test EPUB. Turn ten pages with the page buttons, then ten pages by touch.
- Tap each tab in the bottom bar: Home, Library, Notes and Settings.
- Press Power once to sleep, then press it once to wake the device.
- Note the page you are on. Exit to Nickel from the Lua app's menu. Start "Kaia Reader" again.

Expect:

- The Lua app starts and opens the book.
- Page turns work by button and by touch.
- Each bottom bar tab opens its screen.
- Sleep and wake work, as they did on 2026-10-14.
- After the restart, the Lua app opens its own settings and the page you noted.

If it goes wrong:

- If the launcher shows a crash screen, take a photo.
- Copy `.adds/kaia/crash.log` over USB in the last section.

### 10. How to tell me what you saw

Send the results to the developer through the channel you already use with them.

- Write one line for each step. Give the step number and one word: passed, failed or not done. Add a short note for anything that looked wrong.
- Include the firmware version, the package name, the book name and format, the two times from step 7, and whether USB or a sleep cover was connected.
- Photos are welcome. Useful photos show the screen at the moment something went wrong, both screens in step 2, and any screen that stayed blank. Cover anything private before you send a photo.
- Copy the logs over USB. Connect the Kobo, then copy `.kaia/rust.log`. Copy `.adds/kaia/crash.log` too if the file exists. Copy them before you start Rust again, because the launcher trims the log at each launch.
- Plug in the USB cable and tap "Kaia remote on" in NickelMenu, so the developer can read the logs over Wi-Fi. Do this with Nickel showing. This entry is a test entry from `tools/kaia/device/remote`. It is not in the package, so it appears only if the developer installed it. "Kaia remote on" turns Wi-Fi on and starts an SSH server that accepts keys only. Tap "Kaia remote off" when the developer says the logs are read.
- Do not post serial numbers, network names, passwords or book text in a public place.
