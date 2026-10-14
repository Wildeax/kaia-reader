# Measurement method (KR-006)

Status: method only. Nothing in this document is a measured result. The thresholds in the table at the end are proposed and not measured. The device rig, workload scripts and full approved corpus do not exist yet. The desktop CREngine comparison uses a two-book subset of existing repository fixtures, described below. Build the device rig and full corpus with the baseline before reporting device measurements.

Owner: QA and performance engineer. Depends on the hardware profile (KR-002) and the test scripts and corpus (KR-004 and KR-006).

## Reference conditions

Report three conditions separately. Never merge them into one number.

1. **Stock Nickel.** The firmware from the profile with Nickel as the reader and no KOReader or Kaia installed.
2. **Stock pinned KOReader.** The tagged baseline `v2026.07.1` (commit `9192014d8`), in the release package that matches the firmware (see HARDWARE_PROFILE.md).
3. **Kaia.** A recorded commit of the Kaia build under test.

Compare Kaia mainly with the pinned KOReader. Kaia keeps KOReader's engines and most of its functions, so KOReader is the fair baseline. Nickel is the product reference.

For every run, record the condition, the build identifier or commit, the package name, the firmware and the profile file name.

## Matched settings

Set the same values in every condition. If one condition cannot match a setting, write down what differs.

| Setting | Rule |
|---|---|
| Documents | The same corpus files, identified by SHA-256 (see the corpus section). |
| Typography | Same font, size, margins, line spacing and justification. Where a setting has no equivalent, record the closest value. |
| Frontlight and warmth | Same numeric level in each condition. Nickel and KOReader may map these values differently. Record both. |
| Color rendering | Same on or off state. KOReader switches the framebuffer depth when color rendering is off, so this changes the code path. |
| Wireless | Wi-Fi off, Bluetooth off, in every condition. |
| Auto-suspend and screensaver | Same timers in every condition. |
| Page refresh | Same full-refresh frequency where the setting exists. Record Nickel's behavior if it has no such setting. |
| Battery | Start each controlled discharge at the same range. Record the temperature. |
| Background work | Run the library scan once, wait five minutes, then start timing. Record whether the Kaia index is cold or warm. |
| Input | The same input method for all conditions: the same tap zone, or the same page-turn buttons. Use the rig described below where possible. |
| Restart | Reboot before each session. Start from a fixed page of the same book. |

## Workloads

Each workload is a fixed script. The script names the book, the page, the search term and the number of repetitions. Scripts live in the test tooling, not in product code.

| Workload | Measured quantity | Notes |
|---|---|---|
| Cold open | Time from tap to first page visible | After reboot, the book not in any cache. |
| Warm open | Same, with the book opened once before | Second open in the same session. |
| Page turn | Input to software completion, and input to visible page | 100 forward page turns after 5 warm-up turns. |
| Search | Time to first result and to the end of the result list | One fixed term on one fixed book. |
| Highlight | Time from selection to saved highlight | Ten highlights at fixed positions. |
| Library scan | Time to finish a scan of a fixed folder | Folder size and file count recorded. |
| Suspend and resume | Success count and time to resume | 50 cycles per session for baseline work. The plan asks for 500 cycles before a stable certification. |
| Idle | Wakeups and unexplained screen refreshes in 30 minutes | Count refreshes and wakeups where the device exposes them. |

The plan also lists peak memory, CPU activity, screen refresh count and energy per controlled workload. Record them in every session where the device exposes the value. Note any value that the device does not expose, instead of estimating it.

## Measuring latency: input, software and visible time

Two times matter for each page turn. They are different and must be reported separately.

- **Input to software completion.** Time from the touch or button event to the moment the software marks the page turn complete.
- **Input to visible page.** Time from the same event to the moment the new page is fully on the panel.

The gap between them is the e-ink refresh and the panel waveform. It is not software time.

Three timestamps give both numbers:

1. **Input time (T0).** Use one of two sources:
   - the timestamp of the touch event read from the device input device (the launcher uses `/dev/input/event1` for touch on non-sunxi devices), recorded by a logger on the device, or
   - the rig trigger time, if a rig actuates the screen.
   Calibrate the offset between the two once per rig setup.
2. **Software completion (T1).** The timestamp that Kaia's trace or KOReader's equivalent logs when the page turn finishes. Confirm the trace point exists in the build under test. The plan lists tracing in the Kaia Lua modules.
3. **Visible page (T2).** A high-speed camera (240 frames per second or faster) that films the screen and the rig's trigger LED in one frame. T0 is the frame where the LED lights. T2 is the first frame where the full new page is visible. The frame interval sets the resolution. A photodiode can replace the camera for a region of the screen that changes with each page, but it needs a test patch that a real build does not ship.

Then:

- input to software = T1 minus T0,
- input to visible = T2 minus T0.

Clock alignment between the device log and the camera is the hard part. Use one shared reference per session. For example, the device logs a timestamp when the rig LED lights, and the camera sees the same LED. State the method in each report.

Do not report input to software as "latency" without the visible number. A fast software path with a slow panel still feels slow.

## Controlled discharge

Battery percentage alone does not prove a sleep-current change. The readings are coarse, and short runs mostly measure noise. The plan says the same.

Procedure:

1. Charge the device to 100 percent. Unplug it and wait 30 minutes before starting.
2. Set the matched settings. Turn off Wi-Fi and Bluetooth.
3. Run a fixed workload loop for the whole test. Example: one page turn every 30 seconds, and one suspend and resume every 10 minutes. Use the same loop for every condition.
4. Log the battery reading every 10 minutes. Read the value from the same source each time. Record the source. Find the sysfs path on the device first, because the launcher does not read it.
5. Log the temperature if a sensor is available, or the room temperature.
6. Stop at a fixed charge, for example 20 percent. Do not run to 0 percent.
7. Report the discharge rate in percentage points per hour, with the run-to-run spread.

For energy numbers, use an external measurement: a bench supply in place of the battery, or an inline meter that records current and voltage. Either needs a hardware change and a safety review. Neither is part of the routine method yet.

Discharge runs are long. A suspended device for 24 hours is a useful single run. Plan for days, not hours.

## Repeats and reporting

Proposed numbers, not measured. Calibrate them after the first baseline.

- **Latency:** 3 sessions per condition, with the conditions interleaved (A, B, C, A, B, C ...). Each session has 5 warm-up turns and 100 measured turns, so about 300 turns per condition per workload.
- **Discharge:** at least 3 runs per condition, interleaved in time.
- **Suspend:** 50 cycles per session at baseline, and 500 cycles before any stable claim.

Report these values for each condition and workload:

- **median**,
- **p95**,
- **variability**: the interquartile range across sessions, and the coefficient of variation of the session medians.

Compute p95 from the pooled turns of all sessions. With 100 turns in one session, the p95 is only the fifth-largest value and is too noisy alone.

For a comparison between conditions, report the difference in medians (Kaia minus pinned KOReader) with a 95 percent bootstrap interval, resampling over sessions. Make a claim only when the interval excludes zero and the size of the difference passes the threshold. A single good run is not evidence. Do not describe a change as an improvement without repeatable evidence.

Store each run as a CSV file with one row per measured event. Columns: condition, build, firmware, workload, session, event index, T0, T1, T2 (if measured), and a flag for any event that was retried or dropped.

## Corpus

The corpus must cover the cases below. Use only files with a clear license, and record each file in a manifest. KR-007 requires a license review before any corpus file is bundled in the repository.

| Kind | Needed | Preferred source |
|---|---|---|
| EPUB, reflowable, English | A short novel (about 300 pages), a long novel (1000 or more pages), a verse book, a book with footnotes and images, a book with embedded fonts | Project Gutenberg public-domain EPUBs |
| EPUB, right-to-left | Arabic or Hebrew text | Project Gutenberg, or a text with a clear license |
| EPUB, CJK | Japanese or Chinese text | Project Gutenberg, or a text with a clear license |
| EPUB, malformed | Files with deliberate faults: broken zip, missing manifest entry, invalid XHTML | Generated from a clean file by a project script. License: the project's own. |
| PDF, text | A single-column document and a two-column academic paper | Documents with a stated permissive license (CC BY, CC0, or a US government work that says it is public domain), or PDFs generated from the project's own text |
| PDF, large | At least 500 pages | Generated from the project's own text |
| PDF, fixed layout | A form-like or slide-like document with exact page geometry | Stated permissive license, or project-generated |
| Scanned pages | Image-only PDF pages, a few hundred | Pages the project scans from public-domain material, or a scan with a stated license |
| CBZ | A short comic and a long image set | Comic with a stated license (CC BY or CC0), or project-generated images |
| DjVu | Only if the target build supports it | As for PDF |
| Large library | 500 or more files for library scan cost | Copies of a few clean files under different names. Each copy keeps its original license. Record this in the manifest. |

The desktop rendering comparison in `tools/kaia/cre_compare.py` currently uses `test/juliet.epub` and `test/leaves.epub`. Their manifest is `tools/kaia/cre-corpus/manifest.json`, with hashes, sizes, source URLs and rights evidence from the existing files. It adds no book files or downloads. This subset covers English verse, cover images and long pagination, but it does not satisfy the full corpus table or replace KR-007 review. RTL, CJK, embedded fonts, footnotes and deliberately malformed EPUBs still need approved fixtures. See `docs/kaia/CI.md` for the matched settings, page sampling and image thresholds. These desktop checks report rendering parity, not device latency or energy.

### License rules

- **Project Gutenberg** texts are public domain in the United States. Terms differ in other countries. Keep the Gutenberg license text with the files. Record the ebook number, the source URL and the date of retrieval.
- **PDFs** need a stated permissive license in the document or on its page. Read the license on the source page, not only the file name. Record the SPDX identifier when one exists, such as `CC-BY-4.0` or `CC0-1.0`. For US government works, record the source statement.
- **Scans** are only usable when the scanned material is public domain or licensed for reuse. Record the source of the original.
- **Project-generated files** belong to the project under its own license. Record the generator script and its version.
- Do not use a book purchased from a store or a download of unknown origin. Do not use DRM-protected books. The plan excludes DRM support.

### Manifest

Each corpus file gets one row: file name, size, SHA-256, source URL, license (SPDX or a short statement), date retrieved, number of pages or length, and the features it tests. Keep the manifest with the test scripts. Keep the files outside the repository until KR-007 approves them.

## Initial review thresholds

Proposed, not measured. The plan (section 8) lists these as review gates. They are not claims about the product. Calibrate them against the observed variability of the pinned baseline before any gate fails a build.

| Area | Gate |
|---|---|
| Reading latency | Investigate any repeatable p95 regression above 5% against the pinned baseline. |
| Memory | Investigate growth above 10% on the agreed corpus. Require a hard budget and headroom established from the actual device. |
| Idle | No new periodic application work or unexplained redraws without a feature justification. |
| Suspend | No unexplained drain regression. At least 500 representative suspend and resume cycles before stable certification. |
| Data | No lost committed annotations in the specified fault-injection tests. |
| Soak | At least one week of ordinary reading and sleep use on each certified firmware before release. |

Calibrate statistical thresholds after the baseline exists. Do not fail or advertise improvements based on noise. Never trade meaningful ghosting or annotation loss for a faster synthetic benchmark.
