//! Page turn benchmark for the Rust reader (R1 exit check: under 100 ms on the
//! desktop release build).
//!
//! The app runs on the headless backend with the real loop: the command, the
//! reader job on its worker thread, the 20 ms pump tick, the page widget paint
//! and the present to the display. Each turn is timed from `App::command` to the
//! first step after the reader is idle that presents a new frame. That step is
//! the paint stage. The time before it is the wait stage: the job and the pump
//! tick.
//!
//! The app does not report the stage times of its reader job. So the engine part
//! of each turn runs again on a second book session, after the app turn has
//! finished. The replay opens the same book with the same viewport and layout. It
//! makes the same session calls in the same order as the job in
//! `kaia-app/src/surfaces/reader/work.rs`: go to, render, save, marks, bookmark,
//! transform, links, the sticker and ink store reads, and the ink anchor of the
//! page. Each call has its own time. The sticker text cache and the stroke anchors
//! run only when a book has stickers or ink. The benchmark books have neither, so
//! those paths are not replayed.
//!
//! `wait_residual_ms` is the wait stage minus the replayed engine total. It holds
//! the pump tick (up to 20 ms) and any job work the replay does not cover.
//!
//! Usage. `tools/kaia/bench/rust-page-turn.sh` runs this for each book:
//!
//! ```text
//! cargo run --release -p kaia-app --example page_turn_bench -- \
//!     --book test/juliet.epub --csv OUT/page-turns.csv [--large-font 85]
//! ```
//!
//! Each book runs the default layout. With `--large-font PX` a reflowable book
//! also runs at that font size in pixels. Each run opens the book in a fresh data
//! folder, makes 5 warm-up forward turns, then 50 measured forward turns and 20
//! measured backward turns. The warm-up rows go to the CSV with `phase` set to
//! `warmup`.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use kaia_app::{App, AppOptions, HeadlessBackend, Route, ScriptInput, Services};
use kaia_book::{BookSession, Location, Margins, OpenOptions, Size, TextLayout};
use kaia_display::PanelSpec;
use kaia_events::{Settings, SystemClock, Value};
use kaia_gfx::{Color, Pixmap, Rect};
use kaia_library::{Library, Sort, SortKey};
use kaia_platform::{Env, Platform};
use kaia_storage::Store;

const WARMUP: usize = 5;
const FORWARD: usize = 50;
const BACKWARD: usize = 20;
/// The default longest a single turn may take before the run fails. The first turn of
/// a session also builds CREngine's text index, which takes seconds on long books.
/// Warm-up turns are not measured, so the limit is generous. Set
/// `KAIA_BENCH_TURN_LIMIT_S` to change it.
const TURN_LIMIT_S: u64 = 120;

/// The wall-clock budget for the measured turns of one layout, in seconds. A book whose
/// turns are slow stops early and keeps the turns it has. The warm-up is not limited.
/// Set `KAIA_BENCH_BUDGET_S` to change it.
const BUDGET_S: u64 = 900;

fn budget() -> Duration {
    let secs = std::env::var("KAIA_BENCH_BUDGET_S")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(BUDGET_S);
    Duration::from_secs(secs)
}

fn turn_limit() -> Duration {
    let secs = std::env::var("KAIA_BENCH_TURN_LIMIT_S")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(TURN_LIMIT_S);
    Duration::from_secs(secs)
}
/// KOReader units the app uses for a book with no saved layout. See `koreader_default_layout`.
const FONT_UNITS: f64 = 22.0;
const SIDE_MARGIN_UNITS: f64 = 10.0;
const TOP_MARGIN_UNITS: f64 = 15.0;
/// The app scales units by `ceil(units * min(w, h) / 600)`.
const REFERENCE: f64 = 600.0;

struct Args {
    book: PathBuf,
    csv: PathBuf,
    large_font: Option<f32>,
}

fn parse_args() -> Result<Args, String> {
    let mut book = None;
    let mut csv = None;
    let mut large_font = None;
    let mut it = std::env::args().skip(1);
    while let Some(flag) = it.next() {
        let value = it.next().ok_or_else(|| format!("{flag} needs a value"))?;
        match flag.as_str() {
            "--book" => book = Some(PathBuf::from(value)),
            "--csv" => csv = Some(PathBuf::from(value)),
            "--large-font" => {
                large_font = Some(
                    value
                        .parse::<f32>()
                        .map_err(|e| format!("--large-font {value}: {e}"))?,
                );
            }
            other => return Err(format!("unknown argument {other}")),
        }
    }
    Ok(Args {
        book: book.ok_or("--book is required")?,
        csv: csv.ok_or("--csv is required")?,
        large_font,
    })
}

fn panel() -> PanelSpec {
    PanelSpec::LIBRA_COLOUR
}

/// The layout the app gives a book with no saved state, with the default settings.
fn default_layout(screen: Size) -> TextLayout {
    let px = |units: f64| (units * f64::from(screen.w.min(screen.h)) / REFERENCE).ceil() as f32;
    TextLayout {
        font_size: px(FONT_UNITS),
        margins: Margins {
            top: px(TOP_MARGIN_UNITS),
            right: px(SIDE_MARGIN_UNITS),
            bottom: px(TOP_MARGIN_UNITS),
            left: px(SIDE_MARGIN_UNITS),
        },
        ..TextLayout::default()
    }
}

/// Pins the screens to English, as the headless tests do, so the text does not
/// depend on the machine locale.
fn pin_english(data: &Path) {
    fs::create_dir_all(data).unwrap();
    let path = data.join("settings.json");
    if path.exists() && Settings::read_exact(&path).is_err() {
        return;
    }
    let mut settings = Settings::open(&path);
    if settings.read(kaia_app::i18n::SETTING).as_str().is_none() {
        settings.save(kaia_app::i18n::SETTING, Value::from("en"));
        settings.flush(&path).unwrap();
    }
}

fn make_app(data: &Path, library: &Path) -> App<HeadlessBackend> {
    let root = data.join("platform");
    fs::create_dir_all(&root).unwrap();
    let env = Env {
        home: Some(root.clone()),
        ..Env::default()
    };
    let platform = Platform::detect_in(&root, &env).unwrap();
    pin_english(&data.join("data"));
    let services = Services::open(platform, panel(), Some(data.join("data"))).unwrap();
    let backend = HeadlessBackend::new(panel(), ScriptInput::idle());
    App::new(
        backend,
        services,
        Box::new(SystemClock::new()),
        AppOptions {
            library_roots: vec![library.to_path_buf()],
        },
    )
}

fn wait_until(app: &mut App<HeadlessBackend>, what: &str, done: impl Fn(&App<HeadlessBackend>) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(300);
    while !done(app) {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        app.iterate(Some(Duration::from_millis(5))).unwrap();
    }
}

fn book_id(app: &App<HeadlessBackend>, name: &str) -> i64 {
    let library: &Library = &app.services().library;
    library
        .books(Sort::new(SortKey::Title, false), &Default::default(), 0, 100)
        .unwrap()
        .into_iter()
        .find(|b| b.path.file_name().is_some_and(|f| f == name))
        .unwrap_or_else(|| panic!("{name} is not in the library"))
        .id
}

/// Every eighth pixel of the frame, in row order.
fn sample(frame: &Pixmap) -> Vec<Option<Color>> {
    let mut out = Vec::new();
    for y in (0..frame.height() as i32).step_by(8) {
        for x in (0..frame.width() as i32).step_by(8) {
            out.push(frame.get(x, y));
        }
    }
    out
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

/// The app side of one turn.
struct AppTurn {
    dispatch_ms: f64,
    wait_ms: f64,
    paint_ms: f64,
    end_to_end_ms: f64,
    refreshes: usize,
    refresh_mode: String,
    changed_share: f64,
}

/// Runs one page turn on the app. The command is timed alone. The loop then runs
/// until the reader is idle and the display has a new frame. The step that
/// presents the frame is the paint stage.
fn app_turn(app: &mut App<HeadlessBackend>, command: &str) -> Result<AppTurn, String> {
    let before = sample(app.frame());
    let presented_before = app.backend().display().refreshes().len();
    let started = Instant::now();
    app.command(command, &[]);
    let dispatch = started.elapsed();
    assert!(app.reader_busy(), "{command} did not start a page turn");
    let paint = loop {
        let begin = Instant::now();
        app.process(Vec::new()).unwrap();
        let step = begin.elapsed();
        let presented = app.backend().display().refreshes().len() > presented_before;
        if presented && !app.reader_busy() {
            break step;
        }
        if started.elapsed() >= turn_limit() {
            return Err(format!(
                "{command} did not finish within {:.0} s",
                turn_limit().as_secs_f64()
            ));
        }
        std::thread::sleep(Duration::from_micros(100));
    };
    let end_to_end = started.elapsed();
    let after = sample(app.frame());
    let differing = before.iter().zip(&after).filter(|(a, b)| a != b).count();
    Ok(AppTurn {
        dispatch_ms: ms(dispatch),
        wait_ms: (ms(end_to_end) - ms(dispatch) - ms(paint)).max(0.0),
        paint_ms: ms(paint),
        end_to_end_ms: ms(end_to_end),
        refreshes: app.backend().display().refreshes().len() - presented_before,
        refresh_mode: app
            .refresh_modes()
            .last()
            .map(|m| format!("{m:?}"))
            .unwrap_or_default(),
        changed_share: differing as f64 / before.len().max(1) as f64,
    })
}

/// The engine split of one turn, from the replayed session.
#[derive(Default)]
struct Engine {
    go_to_ms: f64,
    render_ms: f64,
    save_ms: f64,
    marks_ms: f64,
    bookmark_ms: f64,
    transform_ms: f64,
    links_ms: f64,
    store_ms: f64,
    ink_anchor_ms: f64,
    render_cache_hit: bool,
}

impl Engine {
    fn total_ms(&self) -> f64 {
        self.go_to_ms
            + self.render_ms
            + self.save_ms
            + self.marks_ms
            + self.bookmark_ms
            + self.transform_ms
            + self.links_ms
            + self.store_ms
            + self.ink_anchor_ms
    }
}

/// Replays the reader job of one turn on the second session, in the job's order.
/// `store_dir` is the Kaia data folder that the sticker and ink frames open.
fn replay_turn(replay: &mut BookSession, store_dir: &Path, page: u32, viewport: Size) -> Engine {
    let mut e = Engine::default();

    let t = Instant::now();
    replay.go_to(Location::Page(page)).expect("replay go_to");
    e.go_to_ms = ms(t.elapsed());
    assert_eq!(
        replay.current().page,
        page,
        "replay did not reach the turn target"
    );

    let hits_before = replay.cache_stats().hits;
    let t = Instant::now();
    let rendered = replay.render_current(viewport).expect("replay render");
    e.render_ms = ms(t.elapsed());
    e.render_cache_hit = replay.cache_stats().hits > hits_before;
    drop(rendered);

    let t = Instant::now();
    let _ = replay.save();
    e.save_ms = ms(t.elapsed());

    let t = Instant::now();
    replay.annotations_on_page(viewport).expect("replay marks");
    e.marks_ms = ms(t.elapsed());

    let t = Instant::now();
    replay.is_bookmarked().expect("replay bookmark");
    e.bookmark_ms = ms(t.elapsed());

    let t = Instant::now();
    replay.page_transform(viewport).expect("replay page transform");
    e.transform_ms = ms(t.elapsed());

    let t = Instant::now();
    replay.links().expect("replay links");
    e.links_ms = ms(t.elapsed());

    // The sticker frame opens the store, records the document and lists its stickers.
    let t = Instant::now();
    let identity = replay.identity().clone();
    let title = replay.metadata().title.clone();
    let mut store = Store::open(store_dir).expect("replay store");
    store
        .document_seen(&identity.partial_md5, identity.file_size, title.as_deref())
        .expect("replay document seen");
    store
        .list_stickers(&identity.partial_md5)
        .expect("replay stickers");
    e.store_ms = ms(t.elapsed());

    // The ink frame anchors the text position of the page, with or without strokes.
    let t = Instant::now();
    if let Location::Engine(locator) = replay.current().location {
        replay.ink_anchor(&locator).expect("replay ink anchor");
    }
    e.ink_anchor_ms = ms(t.elapsed());

    // The ink frame also reads the strokes of the document.
    let t = Instant::now();
    store.ink(&identity.partial_md5).expect("replay ink strokes");
    e.store_ms += ms(t.elapsed());

    e
}

/// One row of the CSV.
struct Row {
    phase: &'static str,
    direction: &'static str,
    turn: usize,
    page_from: u32,
    page_to: u32,
    app: AppTurn,
    engine: Engine,
}

enum Outcome {
    Measured(Vec<Row>),
    Skipped(String),
}

/// Runs one layout of one book in a fresh data folder.
fn run_layout(book: &Path, work: &Path, large: Option<f32>) -> Result<Outcome, String> {
    let _ = fs::remove_dir_all(work);
    let library = work.join("books");
    fs::create_dir_all(&library).map_err(|e| e.to_string())?;
    let file_name = book.file_name().ok_or("the book has no file name")?.to_owned();
    let copy = library.join(&file_name);
    fs::copy(book, &copy).map_err(|e| e.to_string())?;

    let mut app = make_app(&work.join("data"), &library);
    app.start().map_err(|e| format!("{e:?}"))?;
    wait_until(&mut app, "library scan", |a| !a.scan_running());
    let screen = app.screen();
    let id = book_id(&app, &file_name.to_string_lossy());
    app.command(&format!("open_book:{id}"), &[]);
    wait_until(&mut app, "book open", |a| {
        matches!(a.top_route(), Some(Route::Reader { .. })) && !a.reader_busy()
    });
    app.process(Vec::new()).map_err(|e| format!("{e:?}"))?;

    if let Some(px) = large {
        app.command("reader:layout:size", &[Value::Number(f64::from(px))]);
        app.process(Vec::new()).map_err(|e| format!("{e:?}"))?;
        wait_until(&mut app, "relayout", |a| !a.reader_busy());
        app.process(Vec::new()).map_err(|e| format!("{e:?}"))?;
    }
    let page_rect: Rect = app
        .find_rect("reader-page")
        .ok_or("the reader page widget is not on screen")?;
    let viewport = Size {
        w: page_rect.w,
        h: page_rect.h,
    };

    // The second session. Its data folder is separate, so it shares no state with the app.
    let mut opts = OpenOptions::new(work.join("replay"));
    opts.viewport = viewport;
    opts.pixel_format = app.services().theme.pixel_format();
    opts.default_layout = match large {
        Some(px) => TextLayout {
            font_size: px,
            ..default_layout(screen)
        },
        None => default_layout(screen),
    };
    let mut replay = BookSession::open(&copy, opts).map_err(|e| e.to_string())?;
    let pages = replay.page_count();
    // Forward turns after the warm-up, limited by the book. A short book gets fewer
    // measured turns, and the summary reports the counts.
    let room = pages.saturating_sub(1 + WARMUP);
    if room == 0 {
        return Ok(Outcome::Skipped(format!(
            "{pages} pages at this layout, too few for {WARMUP} warm-up turns and a measured turn"
        )));
    }
    let forward = FORWARD.min(room);
    let backward = BACKWARD.min(WARMUP + forward);
    if !replay.is_reflowable() && large.is_some() {
        return Ok(Outcome::Skipped("fixed layout, so no font size".into()));
    }
    // The job's store path: the parent of the state file's folder, as in `kaia-app`.
    let store_dir = replay
        .state_path()
        .parent()
        .and_then(Path::parent)
        .ok_or("the replay state path has no data folder")?
        .to_path_buf();

    let plan = (0..WARMUP)
        .map(|n| ("warmup", "forward", "reader:next", n))
        .chain((0..forward).map(|n| ("measured", "forward", "reader:next", n)))
        .chain((0..backward).map(|n| ("measured", "backward", "reader:prev", n)));
    let mut page = replay.current().page;
    let mut rows: Vec<Row> = Vec::with_capacity(WARMUP + forward + backward);
    let mut measuring_since: Option<Instant> = None;
    for (phase, direction, command, turn) in plan {
        if phase == "measured" {
            let since = *measuring_since.get_or_insert_with(Instant::now);
            if since.elapsed() > budget() {
                eprintln!(
                    "page_turn_bench: budget reached after {} measured turns, the rest are not run",
                    rows.iter().filter(|r| r.phase == "measured").count()
                );
                break;
            }
        }
        let target = if direction == "forward" {
            page + 1
        } else {
            page - 1
        };
        let app_side = match app_turn(&mut app, command) {
            Ok(turn) => turn,
            Err(reason) => {
                eprintln!("page_turn_bench: stopped at {phase} {direction} {turn}: {reason}");
                break;
            }
        };
        if app_side.end_to_end_ms > 1000.0 {
            eprintln!(
                "page_turn_bench: slow turn {phase} {direction} {turn}: {:.0} ms",
                app_side.end_to_end_ms
            );
        }
        let engine = replay_turn(&mut replay, &store_dir, target, viewport);
        rows.push(Row {
            phase,
            direction,
            turn,
            page_from: page,
            page_to: target,
            app: app_side,
            engine,
        });
        page = target;
    }
    Ok(Outcome::Measured(rows))
}

const HEADER: &str = "book,format,config,phase,direction,turn,page_from,page_to,\
dispatch_ms,wait_ms,paint_ms,end_to_end_ms,refreshes,refresh_mode,changed_share,\
engine_go_to_ms,engine_render_ms,engine_save_ms,engine_marks_ms,engine_bookmark_ms,\
engine_transform_ms,engine_links_ms,engine_store_ms,engine_ink_anchor_ms,engine_total_ms,\
wait_residual_ms,render_cache_hit";

fn write_rows(csv: &Path, book: &str, format: &str, config: &str, rows: &[Row]) -> Result<(), String> {
    let fresh = !csv.exists();
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(csv)
        .map_err(|e| e.to_string())?;
    if fresh {
        writeln!(file, "{HEADER}").map_err(|e| e.to_string())?;
    }
    for r in rows {
        let (a, e) = (&r.app, &r.engine);
        let total = e.total_ms();
        writeln!(
            file,
            "{book},{format},{config},{},{},{},{},{},{:.3},{:.3},{:.3},{:.3},{},{},{:.4},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{}",
            r.phase,
            r.direction,
            r.turn,
            r.page_from,
            r.page_to,
            a.dispatch_ms,
            a.wait_ms,
            a.paint_ms,
            a.end_to_end_ms,
            a.refreshes,
            a.refresh_mode,
            a.changed_share,
            e.go_to_ms,
            e.render_ms,
            e.save_ms,
            e.marks_ms,
            e.bookmark_ms,
            e.transform_ms,
            e.links_ms,
            e.store_ms,
            e.ink_anchor_ms,
            total,
            a.wait_ms - total,
            e.render_cache_hit,
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn nearest_rank(sorted: &[f64], p: f64) -> f64 {
    let rank = ((p * sorted.len() as f64).ceil() as usize).clamp(1, sorted.len().max(1));
    sorted.get(rank - 1).copied().unwrap_or(f64::NAN)
}

fn main() {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("page_turn_bench: {e}");
            std::process::exit(2);
        }
    };
    let book = fs::canonicalize(&args.book).unwrap_or_else(|_| args.book.clone());
    let name = book
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let format = match book
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
    {
        Some(ext) if ext == "epub" || ext == "pdf" => ext,
        _ => "other".to_string(),
    };
    let work_root = std::env::temp_dir().join(format!("kaia-page-turn-{}-{name}", std::process::id()));

    let mut layouts: Vec<(String, Option<f32>)> = vec![("default".into(), None)];
    if let Some(px) = args.large_font {
        layouts.push((format!("large-{px}px"), Some(px)));
    }
    for (label, large) in layouts {
        let started = Instant::now();
        let outcome = run_layout(&book, &work_root.join(&label), large).unwrap_or_else(|e| {
            eprintln!("page_turn_bench: {name} {label} failed: {e}");
            std::process::exit(1);
        });
        match outcome {
            Outcome::Skipped(reason) => {
                eprintln!("page_turn_bench: {name} {label} skipped: {reason}");
            }
            Outcome::Measured(rows) => {
                write_rows(&args.csv, &name, &format, &label, &rows).unwrap_or_else(|e| {
                    eprintln!("page_turn_bench: cannot write {}: {e}", args.csv.display());
                    std::process::exit(1);
                });
                let mut e2e: Vec<f64> = rows
                    .iter()
                    .filter(|r| r.phase == "measured")
                    .map(|r| r.app.end_to_end_ms)
                    .collect();
                e2e.sort_by(f64::total_cmp);
                eprintln!(
                    "page_turn_bench: {name} {label}: {} measured turns, end to end p50 {:.1} ms, p95 {:.1} ms, max {:.1} ms ({:.1} s)",
                    e2e.len(),
                    nearest_rank(&e2e, 0.5),
                    nearest_rank(&e2e, 0.95),
                    e2e.last().copied().unwrap_or(f64::NAN),
                    started.elapsed().as_secs_f64(),
                );
            }
        }
    }
    let _ = fs::remove_dir_all(&work_root);
}
