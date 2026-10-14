//! Desktop, headless and Kobo entry points for the same app loop.

use std::error::Error;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use kaia_app::{App, AppOptions, Backend, HeadlessBackend, ScriptInput, Services};
use kaia_display::PanelSpec;
use kaia_events::SystemClock;
use kaia_platform::Platform;

#[cfg(all(feature = "kobo", target_os = "linux"))]
mod kobo;

type Result<T> = std::result::Result<T, Box<dyn Error>>;

#[derive(Default)]
struct Options {
    headless: bool,
    safe_mode: bool,
    screenshot: Option<PathBuf>,
    data: Option<PathBuf>,
    books: Vec<PathBuf>,
    open: Option<PathBuf>,
    script: Option<PathBuf>,
}

fn main() {
    if let Err(error) = start() {
        eprintln!("kaia: {error}");
        std::process::exit(1);
    }
}

fn start() -> Result<()> {
    let mut options = Options::default();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--safe-mode" => options.safe_mode = true,
            "--headless" => options.headless = true,
            "--screenshot" | "--data-dir" | "--books" | "--open" | "--script" => {
                let value = PathBuf::from(args.next().ok_or_else(|| format!("{arg} needs a path"))?);
                match arg.as_str() {
                    "--screenshot" => options.screenshot = Some(value),
                    "--data-dir" => options.data = Some(value),
                    "--books" => options.books.push(value),
                    "--open" => options.open = Some(std::fs::canonicalize(value)?),
                    _ => options.script = Some(value),
                }
            }
            "--help" | "-h" => {
                println!("Usage: kaia [--headless] [--safe-mode] [--screenshot PNG] [--data-dir DIR] [--books DIR] [--open BOOK] [--script FILE]");
                return Ok(());
            }
            _ => return Err(format!("unknown option: {arg}").into()),
        }
    }
    if options.headless {
        let platform = Platform::detect_in(
            &std::env::temp_dir().join("kaia-no-device"),
            &kaia_platform::Env::from_process(),
        )?;
        let input = match &options.script {
            Some(path) => ScriptInput::from_script(&std::fs::read_to_string(path)?, true)?,
            None => ScriptInput::idle(),
        };
        return run(
            HeadlessBackend::new(PanelSpec::LIBRA_COLOUR, input),
            platform,
            options,
        );
    }
    #[cfg(all(feature = "kobo", target_os = "linux"))]
    {
        kobo::run(options)
    }
    #[cfg(all(feature = "desktop", not(all(feature = "kobo", target_os = "linux"))))]
    {
        let backend = kaia_display::DesktopDisplay::open(
            PanelSpec::LIBRA_COLOUR,
            kaia_display::DesktopOptions::default(),
        )?;
        run(backend, Platform::detect()?, options)
    }
    #[cfg(not(any(feature = "desktop", all(feature = "kobo", target_os = "linux"))))]
    Err("this build requires --headless".into())
}

fn prepare<B: Backend>(backend: B, platform: Platform, options: &Options) -> Result<App<B>> {
    let panel = PanelSpec {
        size: backend.size(),
        dpi: backend.dpi(),
        format: backend.format(),
    };
    let mut services = Services::open(platform, panel, options.data.clone())?;
    // The launcher passes --safe-mode after failed starts. A marker file also turns it on.
    kaia_app::safe_mode::begin(&mut services, options.safe_mode);
    let mut roots = options.books.clone();
    if roots.is_empty() && !options.headless {
        roots.push(services.paths.books_dir.clone());
    }
    if let Some(book) = &options.open {
        let parent = book.parent().ok_or("book has no parent folder")?.to_path_buf();
        if !roots.contains(&parent) {
            roots.push(parent);
        }
    }
    let mut app = App::new(
        backend,
        services,
        Box::new(SystemClock::new()),
        AppOptions { library_roots: roots },
    );
    app.start()?;
    Ok(app)
}

fn open_requested<B: Backend>(app: &mut App<B>, options: &Options) -> Result<()> {
    if let Some(path) = &options.open {
        let book = app
            .services()
            .library
            .book_by_path(path)?
            .ok_or("book was not indexed")?;
        app.command(&format!("open_book:{}", book.id), &[]);
        app.process(Vec::new())?;
        if !matches!(app.top_route(), Some(kaia_app::Route::Reader { .. })) {
            return Err("book did not open".into());
        }
    }
    Ok(())
}

fn run<B: Backend>(backend: B, platform: Platform, options: Options) -> Result<()> {
    let mut app = prepare(backend, platform, &options)?;
    let result = (|| {
        let deadline = Instant::now() + Duration::from_secs(180);
        while app.scan_running() {
            if Instant::now() >= deadline {
                return Err("library scan timed out".into());
            }
            app.process(Vec::new())?;
            std::thread::sleep(Duration::from_millis(20));
        }
        open_requested(&mut app, &options)?;
        if options.script.is_some() || options.screenshot.is_none() {
            while app.iterate(None)? != kaia_app::Flow::Exit {}
        }
        if let Some(path) = &options.screenshot {
            kaia_app::app::save_frame(app.frame(), path)?;
            println!("wrote {}", path.display());
        }
        for notice in app.notices() {
            eprintln!("kaia: {notice}");
        }
        Ok(())
    })();
    app.shutdown();
    result
}
