# KR-006 benchmark baseline (desktop emulator)

Status: desktop emulator baseline, debug build. These numbers are for regression comparison between commits on one machine. They are not device claims. Device numbers come from the procedure in the last section, and the reference conditions are defined in `MEASUREMENT_METHOD.md`.

## Machine and build

- Machine: Apple M5 Pro, macOS 26.3 (Darwin 25.3.0), arm64.
- Build: `koreader-emulator-arm64-apple-darwin25.3.0-debug`, offscreen SDL, 1264 x 1680 at 300 DPI.
- Commit: `b663f3d4b` for all 20 runs (recorded in every run file).

Caveats on the commit:

- The working tree had uncommitted changes during the batch. Among them are `plugins/kaiastickers.koplugin/*`, `frontend/pluginloader.lua`, `frontend/apps/filemanager/filemanagermenu.lua` and `rust/kaia-core/src/*`.
- The emulator loads the Lua sources from the working tree through symlinks (`frontend`, `plugins`, `reader.lua`). `libs/libkaia.dylib` comes from the build directory.
- HEAD moved to `b2db0afc1` and the tree changed again after the batch. The exact tree is not reproducible from the commit alone. Before comparing commits, rerun from a clean worktree.

## Configurations

| Mode | Settings for the run |
|---|---|
| `baseline` | `start_with` set to `filemanager`. `plugins_disabled` has `kaiastickers`. `KAIA_LIBKAIA` points at a path that does not exist. |
| `kaia` | Defaults: Kaia Home at start, all plugins, libkaia loaded. |

Each run file records what the run loaded. Every baseline run has `libkaia: false`, `kaiastickers: false`, `start_with: filemanager`. Every kaia run has `libkaia: true`, `kaiastickers: true`, `start_with: kaia_home`.

The baseline is not stock KOReader. The build still contains the Kaia Home and Notes code, and both modes share the same frontend changes. A true baseline needs the upstream build (`v2026.07.1`, commit `9192014d8`) run on this rig. That is not done yet.

## Workload

Script: `tools/kaia/bench/workload.lua`. It runs the steps below after a 3 second settle and writes JSON.

1. Kaia Home first paint: `show()` to the end of the next repaint.
2. Notes first paint: same method.
3. Cold open of the book: `ReaderUI:showReader()` to the `ReaderReady` event. Each run uses a fresh KO_HOME and its own copy of the book, so sidecars are cold.
4. Page turns: 5 warm-up turns, then 50 measured. A turn runs from the `GotoViewRel` event to the end of the next `UIManager:_repaint`. Every measured turn moved the position (50 of 50 in every run).
5. Search: `ReaderSearch:searchFromStart("the")`, time to the first hit. It is the engine search only. The full result list is not measured.
6. Highlights: 5 per run. Selection and `saveHighlight()` form the create time. `saveSettings()` forms the save time.
7. Memory: `collectgarbage("count")` before and after a full collect. Process RSS from `/proc/self/statm` on Linux, or `ps -o rss= -p $PPID` on macOS.

Timing uses `ui/time` `time.now()`, which is the monotonic coarse clock. The workload records times, counts and memory only. The JSON never holds book text. The search term is a benchmark setting and is recorded.

Not measured by this workload: visible latency (camera), warm open, library scan, suspend and resume, idle wakeups, screen refresh counts, CPU, and controlled discharge.

## How to run on the desktop

```sh
tools/kaia/bench/run.sh ~/kaia-test-library/1342.epub 5 ~/kaia-bench/2026-10-13
tools/kaia/bench/run.sh ~/kaia-test-library/84.epub 5 ~/kaia-bench/2026-10-13
python3 -I tools/kaia/bench/summarize.py ~/kaia-bench/2026-10-13 --out summary.md
```

`KAIA_BENCH_MODE=baseline` or `kaia` runs one mode only. Unset, both modes run interleaved. `KAIA_BENCH_SEARCH` changes the term. `KAIA_BENCH_RUN_TIMEOUT` sets the per-run limit in seconds.

The raw run files (20 JSON files and the run log) sit outside the repository, in `~/kaia-bench/2026-10-13/`.

## Results: 5 runs per mode, debug build, commit b663f3d4b

Columns: Median is the median of the run values (for page turns and highlights, the median of run medians). p95 is the nearest-rank p95. For page turns and highlights it is pooled over all runs. For scalar metrics it is the p95 of the run values. With 5 runs that is the largest value. Spread is (max - min) / median of the run values.

### 1342.epub (24 MB, with images)

| Metric | Mode | Runs | Median | p95 | Min | Max | Spread |
|---|---|---:|---:|---:|---:|---:|---:|
| kaia_home_first_paint_ms | baseline | 5 | 6.4 | 11.8 | 4.6 | 11.8 | 113.1% |
| kaia_home_first_paint_ms | kaia | 5 | 13.6 | 15.1 | 10.1 | 15.1 | 36.9% |
| notes_first_paint_ms | baseline | 5 | 4.2 | 5.5 | 3.1 | 5.5 | 57.3% |
| notes_first_paint_ms | kaia | 5 | 6.6 | 7.6 | 6.0 | 7.6 | 24.7% |
| cold_open_ms | baseline | 5 | 157.7 | 165.3 | 153.9 | 165.3 | 7.2% |
| cold_open_ms | kaia | 5 | 162.3 | 165.0 | 160.1 | 165.0 | 3.0% |
| page_turn_ms (p95 pooled over runs) | baseline | 5 | 3.5 | 6.0 | 3.1 | 3.5 | 13.6% |
| page_turn_ms (p95 pooled over runs) | kaia | 5 | 3.5 | 5.8 | 3.4 | 3.7 | 7.3% |
| search_first_ms | baseline | 5 | 2.4 | 2.5 | 2.4 | 2.5 | 4.8% |
| search_first_ms | kaia | 5 | 2.4 | 2.6 | 2.4 | 2.6 | 9.3% |
| search_found | baseline | 5 | 1.00 | 1.00 | 1.00 | 1.00 | 0.0% |
| search_found | kaia | 5 | 1.00 | 1.00 | 1.00 | 1.00 | 0.0% |
| highlight_create_ms (p95 pooled over runs) | baseline | 5 | 0.4 | 0.5 | 0.2 | 0.5 | 74.2% |
| highlight_create_ms (p95 pooled over runs) | kaia | 5 | 0.4 | 0.6 | 0.4 | 0.5 | 31.9% |
| highlight_save_ms (p95 pooled over runs) | baseline | 5 | 1.6 | 2.4 | 1.3 | 1.9 | 40.0% |
| highlight_save_ms (p95 pooled over runs) | kaia | 5 | 1.7 | 2.4 | 1.7 | 1.9 | 13.0% |
| lua_kb | baseline | 5 | 13,415 | 14,037 | 11,394 | 14,037 | 19.7% |
| lua_kb | kaia | 5 | 14,279 | 14,492 | 13,273 | 14,492 | 8.5% |
| lua_kb_after_gc | baseline | 5 | 9,876.90 | 10,518.48 | 9,788.86 | 10,518.48 | 7.4% |
| lua_kb_after_gc | kaia | 5 | 9,935.56 | 10,033.67 | 9,875.81 | 10,033.67 | 1.6% |
| rss_kb | baseline | 5 | 168,400 | 170,016 | 167,744 | 170,016 | 1.3% |
| rss_kb | kaia | 5 | 169,744 | 171,008 | 169,040 | 171,008 | 1.2% |

| Metric | Baseline median | Kaia median | Median diff | Baseline p95 | Kaia p95 | p95 diff | Check | Result |
|---|---:|---:|---:|---:|---:|---:|---|---|
| kaia_home_first_paint_ms | 6.4 | 13.6 | +113.8% | 11.8 | 15.1 | +28.0% | p95 | REVIEW: kaia p95 +28.0% (gate 5%) |
| notes_first_paint_ms | 4.2 | 6.6 | +54.8% | 5.5 | 7.6 | +37.7% | p95 | above 5% gate, inside baseline range (noise?) |
| cold_open_ms | 157.7 | 162.3 | +2.9% | 165.3 | 165.0 | -0.2% | p95 | ok |
| page_turn_ms | 3.5 | 3.5 | +1.0% | 6.0 | 5.8 | -3.4% | p95 | ok |
| search_first_ms | 2.4 | 2.4 | -0.2% | 2.5 | 2.6 | +3.6% | p95 | ok |
| search_found | 1.00 | 1.00 | +0.0% | 1.00 | 1.00 | +0.0% | - | not gated |
| highlight_create_ms | 0.4 | 0.4 | +6.5% | 0.5 | 0.6 | +4.2% | p95 | ok |
| highlight_save_ms | 1.6 | 1.7 | +5.1% | 2.4 | 2.4 | -0.2% | p95 | ok |
| lua_kb | 13,415 | 14,279 | +6.4% | 14,037 | 14,492 | +3.2% | median | ok |
| lua_kb_after_gc | 9,876.90 | 9,935.56 | +0.6% | 10,518.48 | 10,033.67 | -4.6% | - | not gated |
| rss_kb | 168,400 | 169,744 | +0.8% | 170,016 | 171,008 | +0.6% | median | ok |

### 84.epub (475 KB)

| Metric | Mode | Runs | Median | p95 | Min | Max | Spread |
|---|---|---:|---:|---:|---:|---:|---:|
| kaia_home_first_paint_ms | baseline | 5 | 7.6 | 11.4 | 3.3 | 11.4 | 105.6% |
| kaia_home_first_paint_ms | kaia | 5 | 8.6 | 11.1 | 5.3 | 11.1 | 67.0% |
| notes_first_paint_ms | baseline | 5 | 4.7 | 6.3 | 3.3 | 6.3 | 63.5% |
| notes_first_paint_ms | kaia | 5 | 6.9 | 7.9 | 5.1 | 7.9 | 40.5% |
| cold_open_ms | baseline | 5 | 84.8 | 89.2 | 76.2 | 89.2 | 15.3% |
| cold_open_ms | kaia | 5 | 86.5 | 87.9 | 78.0 | 87.9 | 11.5% |
| page_turn_ms (p95 pooled over runs) | baseline | 5 | 3.1 | 3.8 | 2.9 | 3.4 | 16.4% |
| page_turn_ms (p95 pooled over runs) | kaia | 5 | 3.2 | 4.3 | 3.0 | 3.3 | 7.0% |
| search_first_ms | baseline | 5 | 0.2 | 0.2 | 0.2 | 0.2 | 13.1% |
| search_first_ms | kaia | 5 | 0.2 | 0.2 | 0.2 | 0.2 | 2.5% |
| search_found | baseline | 5 | 1.00 | 1.00 | 1.00 | 1.00 | 0.0% |
| search_found | kaia | 5 | 1.00 | 1.00 | 1.00 | 1.00 | 0.0% |
| highlight_create_ms (p95 pooled over runs) | baseline | 5 | 0.4 | 0.6 | 0.3 | 0.5 | 42.4% |
| highlight_create_ms (p95 pooled over runs) | kaia | 5 | 0.4 | 0.6 | 0.3 | 0.5 | 31.4% |
| highlight_save_ms (p95 pooled over runs) | baseline | 5 | 1.7 | 2.7 | 1.7 | 1.9 | 10.1% |
| highlight_save_ms (p95 pooled over runs) | kaia | 5 | 1.7 | 2.7 | 1.6 | 1.8 | 10.6% |
| lua_kb | baseline | 5 | 13,649 | 14,240 | 12,475 | 14,240 | 12.9% |
| lua_kb | kaia | 5 | 13,975 | 14,188 | 13,305 | 14,188 | 6.3% |
| lua_kb_after_gc | baseline | 5 | 9,852.89 | 9,920.72 | 9,787.65 | 9,920.72 | 1.4% |
| lua_kb_after_gc | kaia | 5 | 9,946.09 | 10,075.26 | 9,876.81 | 10,075.26 | 2.0% |
| rss_kb | baseline | 5 | 142,672 | 145,616 | 142,400 | 145,616 | 2.3% |
| rss_kb | kaia | 5 | 144,560 | 145,024 | 143,744 | 145,024 | 0.9% |

| Metric | Baseline median | Kaia median | Median diff | Baseline p95 | Kaia p95 | p95 diff | Check | Result |
|---|---:|---:|---:|---:|---:|---:|---|---|
| kaia_home_first_paint_ms | 7.6 | 8.6 | +12.4% | 11.4 | 11.1 | -2.8% | p95 | ok |
| notes_first_paint_ms | 4.7 | 6.9 | +46.8% | 6.3 | 7.9 | +25.7% | p95 | above 5% gate, inside baseline range (noise?) |
| cold_open_ms | 84.8 | 86.5 | +2.0% | 89.2 | 87.9 | -1.4% | p95 | ok |
| page_turn_ms | 3.1 | 3.2 | +2.2% | 3.8 | 4.3 | +12.4% | p95 | above 5% gate, inside baseline range (noise?) |
| search_first_ms | 0.2 | 0.2 | -1.9% | 0.2 | 0.2 | -8.1% | p95 | ok |
| search_found | 1.00 | 1.00 | +0.0% | 1.00 | 1.00 | +0.0% | - | not gated |
| highlight_create_ms | 0.4 | 0.4 | +0.2% | 0.6 | 0.6 | +4.3% | p95 | ok |
| highlight_save_ms | 1.7 | 1.7 | -4.3% | 2.7 | 2.7 | +1.1% | p95 | ok |
| lua_kb | 13,649 | 13,975 | +2.4% | 14,240 | 14,188 | -0.4% | median | ok |
| lua_kb_after_gc | 9,852.89 | 9,946.09 | +0.9% | 9,920.72 | 10,075.26 | +1.6% | - | not gated |
| rss_kb | 142,672 | 144,560 | +1.3% | 145,616 | 145,024 | -0.4% | median | ok |

## Reading the comparison

Against the proposed gates (latency p95 above 5%, memory above 10%):

- Memory is under the gate on both books. Lua heap median is +6.4% (1342) and +2.4% (84). RSS is within +1.3%.
- Cold open, search, highlight save, and the 1342 page-turn p95 pass.
- Four rows fail the latency gate on p95, and each is inside the baseline range except one:
  - `kaia_home_first_paint_ms` on 1342: p95 +28.0%, REVIEW. The gap is about 7 ms between medians.
  - `notes_first_paint_ms` on 1342: p95 +37.7%, inside the baseline range.
  - `notes_first_paint_ms` on 84: p95 +25.7%, inside the baseline range.
  - `page_turn_ms` on 84: p95 +12.4%, inside the baseline range.

Caveats for these flags:

- Desktop noise can exceed both gates. A 4 to 8 ms metric moves by 1 to 3 ms from scheduler jitter alone, which is a large percentage.
- The Kaia Home measurement starts from a different state in each mode. In kaia mode, Home is already on screen at startup, and the workload closes it before measuring. In baseline mode, Home is not shown. The workload does not control for this, so the Home difference is not yet a clean comparison. The cause is not diagnosed.
- With 5 runs, the scalar p95 is the largest of 5 values. It is a weak estimate.
- None of these numbers shows an improvement. Nothing here supports a claim that Kaia is faster or slower on a device.

## Variability observed

- Cold open spread: 3.0% to 15.3% across runs.
- Page-turn run medians spread 7% to 16%. The turns are 3 to 6 ms, close to the scheduler jitter floor.
- Highlight create and save spread 10% to 74%. The absolute values are under 3 ms.
- Notes and Kaia Home first paint spread 25% to 113%. The absolute values are 3 to 15 ms.
- Lua heap spread 6% to 20%. RSS spread is 0.9% to 2.3%.

## Gaps before any device comparison

- Visible latency is not measured. The desktop has no camera or photodiode step.
- Warm open, library scan, suspend and resume, idle, screen refresh count, CPU and discharge are not in the workload.
- The baseline is not stock KOReader (see Configurations).
- The Home and Notes start state is not matched between modes (see Reading the comparison).
- The batch ran on a dirty working tree (see Machine and build).
- Only one search term on one book, and the search measures the engine first hit only.

## Running on a Kobo

The workload is the same file. A device has no shell environment, so settings come from a flag file. The environment variable wins if both are set.

1. Package the build under test. Kaia installs under `.adds/kaia`. Stock KOReader installs under `.adds/koreader`.
2. Copy the script into the patches directory with the late priority prefix. `frontend/userpatch.lua` runs files whose names start with `2-` after the UI is up.

   ```sh
   cp tools/kaia/bench/workload.lua /mnt/onboard/.adds/kaia/patches/2-kaia-bench.lua
   ```

   For stock KOReader, use `/mnt/onboard/.adds/koreader/patches/2-kaia-bench.lua`.

3. Create `kaia-bench.conf` in the data directory. The script reads `DataStorage:getDataDir()`. Check the path on the device before the first run. For a Kaia install it should be the `.adds/kaia` root.

   ```text
   KAIA_BENCH_OUT=/mnt/onboard/kaia-bench/kaia-run1.json
   KAIA_BENCH_BOOK=/mnt/onboard/books/1342.epub
   KAIA_BENCH_MODE=kaia
   KAIA_BENCH_RUN=1
   KAIA_BENCH_COMMIT=<short sha>
   KAIA_BENCH_SEARCH=the
   ```

   Create the output directory first. The script does not create it.

4. Reboot, then let the device settle. The patch runs at every start while `kaia-bench.conf` exists. Each start writes the JSON and quits KOReader. Delete the conf file or the patch after the last run. Creating a file named `.patches_disabled` in the patches directory turns all user patches off.
5. Copy the JSON off the device and run `summarize.py` on it.
6. For stock KOReader, use the same patch in `.adds/koreader/patches/`, with `KAIA_BENCH_MODE=stock` in the conf file. The Kaia-only metrics (`kaia_home_first_paint_ms`, `notes_first_paint_ms`) are absent because stock has no Kaia modules, and `config.libkaia` is `false`.
7. Match the settings in `MEASUREMENT_METHOD.md` in every condition: same file (checked by SHA-256), same font, light and warmth, color rendering, Wi-Fi and Bluetooth off, and a reboot before each session.

The JSON gives software completion only (the `page_turn_ms` values and the open and paint times). Visible latency needs the camera method in `MEASUREMENT_METHOD.md`. Film the screen and the rig LED at 240 frames per second or faster. Use the LED frame as T0 and the first frame with the full new page as T2. Calibrate the clock offset once per rig. Do not report software time as latency alone.

Device runs follow the same thresholds, but they are a separate series. Do not compare a debug-build desktop number with a device number.

## Rust app, desktop release

Status: partial measurement of the R1 page turn exit check (REWRITE_PLAN.md R1: page turn under 100 ms on the desktop release build). The check fails. Every EPUB that completed a run has a page turn of about 250 to 320 ms at p50, and the reader's ink anchor call takes most of that time. Rendering takes about 2 ms. The check should be rerun after the ink anchor is fixed. The raw files sit outside the repository, as the Lua results do: `~/kaia-bench/rust-page-turn-2026-10-14-partial/` holds `page-turns.csv`, `run.txt` and `summary.md`.

### Machine and build

- Machine: Apple M5 Pro, 18 cores, 24 GB memory, macOS 26.3 (25D2125), arm64. Headless backend, Libra Colour panel spec, 1264 x 1680.
- Commit: `61a72323e`. The working tree had 20 changed paths from other work in progress (library and settings surfaces in `kaia-app`, `kaia-book/src/state.rs`, `kaia-library`). The binary was built from that tree, so it includes those edits.
- Build: `cargo build --release -p kaia-app --example page_turn_bench`, rustc 1.99.0, release profile as committed (`opt-level = "s"`, `lto = "thin"`, `panic = "unwind"`), `kaia-app` default features (desktop, lua).
- Engine libraries: MuPDF and CREngine come from KOReader's debug staging (`base/build/arm64-apple-darwin25.3.0-debug`), built with `-Og -g`. These numbers therefore do not use the engine's release flags. A release staging was not built, so the engine part of the cost may be lower with it.
- The binary runs from `resources/`, because MuPDF opens its fonts from `./fonts`.

### Method

- Harness: `rust/kaia-app/examples/page_turn_bench.rs`. Runner: `tools/kaia/bench/rust-page-turn.sh`, with `--quick` (juliet and paper) or `--books LIST`. Summary: `tools/kaia/bench/rust_page_turn_summary.py`.
- Timed span: `App::command("reader:next")` or `reader:prev`, to the first app step after the reader is idle that presents a new frame to the headless display. The step that presents the frame is the paint stage. The time before it is the wait stage: the reader job on its worker thread plus the 20 ms pump tick.
- Engine split: the app does not report its job stages, so a second session on the same book replays the job's calls in the job's order: go to, render, save, marks, bookmark, page transform, links, sticker and ink store reads, ink anchor. The replay is an estimate. `wait_residual_ms` is the wait stage minus the replayed total, and it holds the pump tick and any job work the replay does not cover.
- Plan: 5 warm-up forward turns (not in the statistics), then 50 measured forward and 20 measured backward turns, at the default layout. Reflowable EPUBs also run at 85 px (the largest font the Text settings allow at this panel). The default layout is KOReader's 22 units scaled to 47 px, with margins of 22 and 32 px.
- Limits: a turn over 120 s stops that book and keeps its turns. Each layout stops measuring after 900 s of wall time. Books with too few pages run fewer measured turns, and the summary lists them.
- Statistics: nearest-rank p50, p95 and max over measured turns.

### Results (partial run, end to end, ms)

| Book | Config | Direction | Turns | p50 | p95 | Max | Under 100 ms |
|---|---|---|---:|---:|---:|---:|---|
| juliet.epub (EPUB) | default | forward | 50 | 286.4 | 312.9 | 316.7 | no |
| juliet.epub | default | backward | 20 | 270.0 | 282.1 | 295.8 | no |
| juliet.epub | 85 px | forward | 50 | 306.7 | 321.2 | 331.5 | no |
| juliet.epub | 85 px | backward | 20 | 298.4 | 305.4 | 314.2 | no |
| 11.epub (EPUB) | default | forward | 50 | 268.2 | 314.3 | 320.3 | no |
| 11.epub | default | backward | 20 | 244.1 | 260.1 | 260.5 | no |
| 11.epub | 85 px | forward | 50 | 299.4 | 320.2 | 330.9 | no |
| 11.epub | 85 px | backward | 20 | 288.3 | 301.2 | 306.1 | no |
| leaves.epub (EPUB) | default | forward | 10 | 30964.3 | 31319.2 | 31319.2 | no |
| leaves.epub | 85 px | forward | 10 | 31056.6 | 31322.2 | 31322.2 | no |
| paper.pdf (PDF, 22 pages) | default | forward | 16 | 13.4 | 37.9 | 37.9 | yes |
| paper.pdf | default | backward | 20 | 14.3 | 21.4 | 21.6 | yes |
| sample.pdf (PDF, 45 pages) | default | forward | 39 | 18.5 | 197.7 | 298.0 | no |
| sample.pdf | default | backward | 20 | 25.4 | 155.1 | 205.0 | no |

Pooled over the EPUB default runs (juliet and 11, 140 forward and backward turns): p50 271 ms, p95 313 ms, max 320 ms.

leaves.epub stopped after 10 measured turns in each layout, when one turn passed the 120 s limit. Every turn of leaves takes about 31 s. Its first warm-up turn took 31 s too, so this is not a one-time index cost.

### Where the time goes (default layout, p50 per turn, ms)

| Book | Wait (job plus pump) | Paint | Engine render | Engine save | Ink anchor | Replayed engine total |
|---|---:|---:|---:|---:|---:|---:|
| juliet.epub | 275.1 | 2.6 | 2.0 | 12.1 | 239.5 | 255.4 |
| 11.epub | 255.0 | 2.5 | 2.5 | 12.5 | 223.2 | 239.2 |
| leaves.epub | 30961.3 | 3.1 | 0.2 | 13.4 | 30200.0 | 30217.1 |
| paper.pdf | 11.9 | 2.3 | 1.1 | 11.2 | 0.0 | 12.5 |
| sample.pdf | 22.0 | 2.4 | 10.5 | 12.0 | 0.0 | 22.3 |

The command call takes under 0.1 ms. The pump tick adds up to 20 ms to the wait. Paint, which covers the page update and the present, takes 2 to 3 ms at p50.

### Pass or fail

The check fails. EPUB page turns take 250 to 320 ms at p50 and p95 for juliet and 11, and leaves takes about 31 s. Only paper.pdf passes, with a p95 of 38 ms. sample.pdf fails at p95 (198 ms) for the reasons below.

### The dominant stage: ink anchoring

On every turn of a reflowable book, the reader job calls `BookSession::ink_anchor` for the current page. It runs in `kaia-app/src/surfaces/reader/ink.rs` (`frame`), which `work.rs` calls for each turn. The call happens even when the page has no ink strokes, and the origin it returns is used only for stroke placement. Inside the book engine, `ink_anchor` calls CREngine `char_boxes`. A sample of the leaves run during a turn showed the engine thread in `ldomNode::getChildIndex`, `ldomXPointer::getRect` and `ldomXPointerEx::initIndex`, under `kaia_cre_char_boxes`. Rendering the page takes about 2 ms, so the check cannot pass until this call is skipped or cached. The fix belongs to the ink code and was not made in this task.

Other findings from the same run:

- The state save runs on every turn and costs about 12 ms.
- Backward turns in PDFs miss the page cache. Prerendering only goes forward, so a backward PDF turn renders the page again: 140 to 160 ms in sample.pdf.
- Some forward turns in sample.pdf show a wait residual of 100 to 280 ms, even when the replayed render was a cache hit. The replay does not cover that time. A likely cause is the app session's background prerender holding the engine thread between jobs. That is a hypothesis and was not verified.

### Not measured

- 1342.epub, 1661.epub, 2701.epub, 345.epub and 84.epub. The run was stopped before they finished, so they have no rows.
- 2col.pdf (2 pages) and tall.pdf (1 page). Too few pages for the warm-up.
- The 85 px run for PDFs. They are fixed layout, so the font setting does not apply.
- Engine release flags, the Kobo panel, and the e-ink refresh time. The headless present is the end point, so the visible time on the panel is not included.

### How to rerun

`tools/kaia/bench/rust-page-turn.sh --quick` runs juliet and paper in about 2 minutes. `tools/kaia/bench/rust-page-turn.sh --books 84,11` runs chosen books. The full corpus is slow because of leaves and 1342. Pass an output folder as the last argument to keep results apart from the default folder.
