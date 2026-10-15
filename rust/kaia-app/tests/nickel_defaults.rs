//! Fresh installs use the capture-calibrated Nickel fallbacks; saved choices take precedence.

mod common;

use common::*;
use kaia_app::surfaces::reader::page::PageView;
use kaia_app::{App, HeadlessBackend};
use kaia_doc::{cre::CreDocument, Document, Layout, Margins};
use kaia_events::{Event, Settings, Value};
use kaia_gfx::golden::assert_matches_golden;
use std::path::Path;

fn start(dir: &Path, name: &str) -> App<HeadlessBackend> {
    let books = library_with_books(dir);
    if name != "juliet.epub" {
        std::fs::rename(books.join("juliet.epub"), books.join(name)).unwrap();
    }
    let mut app = make_app(dir, vec![books]);
    app.start().unwrap();
    wait_for(&mut app, "library scan", |a| !a.scan_running());
    let id = book_id(&app, name);
    app.command(&format!("open_book:{id}"), &[]);
    wait_reader(&mut app);
    app
}

fn saved_layout(app: &mut App<HeadlessBackend>, name: &str) -> Value {
    app.broadcast_app(Event::new("SaveState"));
    let book = app.services().library.book(book_id(app, name)).unwrap().unwrap();
    let path = app
        .services()
        .paths
        .data_dir
        .join("kaia/books")
        .join(format!("{}.json", book.partial_md5));
    Settings::read_exact(&path)
        .unwrap()
        .unwrap()
        .read("layout")
        .clone()
}

fn expect_layout(layout: &Value, size: f64, side: f64, spacing: f64) {
    assert_eq!(layout.get("font_size").as_f64(), Some(size));
    assert_eq!(layout.get("margins").get("left").as_f64(), Some(side));
    assert_eq!(layout.get("margins").get("right").as_f64(), Some(side));
    assert!((layout.get("line_spacing").as_f64().unwrap() - spacing).abs() < 0.00001);
}

fn check_capture_page(app: &mut App<HeadlessBackend>, name: &str) {
    let saved = saved_layout(app, name);
    assert_eq!(saved.get("hyphenation").as_bool(), Some(false));
    assert_eq!(saved.get("justify").as_bool(), Some(false));
    assert_eq!(saved.get("embedded_styles").as_bool(), Some(true));
    assert_eq!(saved.get("margins").get("top").as_f64(), Some(64.0));
    assert_eq!(saved.get("margins").get("bottom").as_f64(), Some(64.0));
    app.update_top::<PageView, _>("reader-page", |page, _| {
        assert_eq!(page.header_height, 120);
        assert_eq!(page.footer_height, 120);
        assert_eq!(page.position, Some((5, 1090)));
        assert_eq!(page.header, (String::new(), "About · 1 of 2".into()));
        assert_eq!(page.footer, "Romeo and Juliet · Page 3 of 96");
    });

    // Extract visual lines from the same document layout used by the headless app.
    let mut doc = CreDocument::open(&repo_root().join("test/juliet.epub")).unwrap();
    let mut layout = Layout::new(1264.0, 1440.0, 76.0);
    layout.line_height = Some(1.35);
    layout.margins = Some(Margins {
        top: 64.0,
        bottom: 64.0,
        left: 71.0,
        right: 71.0,
    });
    doc.set_layout(&layout).unwrap();
    doc.set_hyphenation(None).unwrap();
    let text = doc.page_text(5).unwrap();
    let lines: Vec<_> = text.text.lines().map(str::trim).collect();
    let expected = [
        "translated into every major",
        "living language, and are",
        "performed more often",
        "than those of any other",
        "playwright. Shakespeare",
        "was born and raised in",
        "Stratford-upon-Avon. At",
        "the age of 18 he married",
        "Anne Hathaway, who bore",
    ];
    assert_eq!(lines, expected);
    assert_eq!(text.lines.len(), 9);
    for pair in text.lines.windows(2) {
        assert_eq!(pair[1].bbox.y0 - pair[0].bbox.y0, 140.0);
    }
    assert_eq!(text.lines[0].bbox.x0, 147.0);
    assert_eq!(doc.page_count(), 1090);
    assert!(doc.page_text(6).unwrap().text.starts_with("him three children:"));
    println!("{name}: 1090 rendered pages; body pitch 140 px; layout inset 147 px");
    for (i, line) in lines.iter().enumerate() {
        println!("{}: {line}", i + 1);
    }
}

#[test]
fn fresh_app_opens_juliet_with_nickel_epub_defaults() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("nickel-epub-defaults");
    let mut app = start(&dir, "juliet.epub");
    assert_eq!(app.screen(), kaia_gfx::Size::new(1264, 1680));
    expect_layout(&saved_layout(&mut app, "juliet.epub"), 76.0, 71.0, 1.35);
    assert!(app
        .services()
        .settings
        .read("reading.font_size")
        .as_f64()
        .is_none());
    app.command("reader:location", &[Value::Number(5.0)]);
    wait_reader(&mut app);
    app.update_top::<PageView, _>("reader-page", |page, _| {
        assert!(page.page.is_some(), "CREngine rendered the fresh book");
        assert!(page.message.is_none());
        println!("fresh EPUB page/count: {:?}", page.position);
    });
    check_capture_page(&mut app, "juliet.epub");
    assert_matches_golden(app.frame(), &golden("reader-nickel-defaults.png"), 14);
}

#[test]
fn fresh_kepub_uses_the_same_layout_as_epub() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("nickel-kepub-defaults");
    let mut app = start(&dir, "juliet.kepub.epub");
    expect_layout(&saved_layout(&mut app, "juliet.kepub.epub"), 76.0, 71.0, 1.35);
    app.command("reader:location", &[Value::Number(5.0)]);
    wait_reader(&mut app);
    check_capture_page(&mut app, "juliet.kepub.epub");
    assert_matches_golden(app.frame(), &golden("reader-nickel-kepub-defaults.png"), 14);
}

#[test]
fn saved_global_and_per_book_values_survive_the_new_defaults() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("nickel-saved-defaults");
    let path = dir.join("data/settings.json");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut settings = Settings::open(&path);
    for (key, value) in [
        ("reading.font_size", 24.0),
        ("reading.margin_left", 12.0),
        ("reading.margin_right", 12.0),
        ("reading.line_spacing", 1.1),
        ("device.sleep_minutes", 30.0),
    ] {
        settings.save(key, Value::Number(value));
    }
    settings.save("reading.header_chapter", Value::Bool(false));
    settings.save("device.sleep_screen", Value::from("cover"));
    settings.save("library.sort", Value::from("title"));
    settings.flush(&path).unwrap();
    let mut app = start(&dir, "juliet.epub");
    expect_layout(&saved_layout(&mut app, "juliet.epub"), 51.0, 26.0, 1.1);
    app.command("reader:layout:size", &[Value::Number(60.0)]);
    wait_reader(&mut app);
    expect_layout(&saved_layout(&mut app, "juliet.epub"), 60.0, 26.0, 1.1);
    app.command("reader:layout:hyphenation", &[Value::Bool(true)]);
    app.command("reader:layout:justify", &[Value::Bool(true)]);
    wait_reader(&mut app);
    app.shutdown();
    drop(app);

    // A new global choice must not replace the book's own saved layout.
    let mut settings = Settings::open(&path);
    settings.save("reading.font_size", Value::Number(36.0));
    settings.flush(&path).unwrap();
    let mut app = start(&dir, "juliet.epub");
    expect_layout(&saved_layout(&mut app, "juliet.epub"), 60.0, 26.0, 1.1);
    let layout = saved_layout(&mut app, "juliet.epub");
    assert_eq!(layout.get("hyphenation").as_bool(), Some(true));
    assert_eq!(layout.get("justify").as_bool(), Some(true));
    let stored = Settings::read_exact(&path).unwrap().unwrap();
    assert_eq!(stored.read("device.sleep_minutes").as_f64(), Some(30.0));
    assert_eq!(stored.read("device.sleep_screen").as_str(), Some("cover"));
    assert_eq!(stored.read("library.sort").as_str(), Some("title"));
    assert_eq!(stored.read("reading.header_chapter").as_bool(), Some(false));
    app.update_top::<PageView, _>("reader-page", |page, _| assert_eq!(page.header_height, 0));
}

#[test]
fn reader_menu_and_hold_use_logical_labels_and_compact_controls() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("nickel-menu-selection");
    let mut app = start(&dir, "juliet.epub");
    app.command("reader:location", &[Value::Number(5.0)]);
    wait_reader(&mut app);
    let reading = app.frame().clone();
    app.command("reader:overlay", &[]);
    wait_reader(&mut app);
    assert_eq!(app.find_rect("overlay-slider").unwrap().y, 1464);
    let mut label = String::new();
    app.update_top::<kaia_ui::components::Label, _>("overlay-footer", |text, _| label = text.text().into());
    assert_eq!(label, "Page 3 of 96");
    app.update_top::<kaia_ui::components::Label, _>("overlay-clock", |text, cx| {
        text.set_text("09:41");
        cx.request_layout();
        cx.request_paint();
    });
    app.iterate(None).unwrap();
    for y in 180..1344 {
        for x in 0..1264 {
            assert_eq!(
                app.frame().get(x, y),
                reading.get(x, y),
                "menu changed page pixel {x},{y}"
            );
        }
    }
    assert_matches_golden(app.frame(), &golden("reader-nickel-menu.png"), 14);
    app.key(kaia_input::Key::Back).unwrap();
    wait_reader(&mut app);
    app.command("reader:hold", &[Value::Number(800.0), Value::Number(830.0)]);
    wait_reader(&mut app);
    app.command("reader:release", &[]);
    wait_reader(&mut app);
    assert_eq!(app.find_rect("sel-highlight").unwrap().w, 322);
    assert_eq!(app.find_rect("dict-body").unwrap().y, 1185);
    assert!(app.find_rect("dict-downloads").is_some());
    assert_matches_golden(app.frame(), &golden("reader-nickel-selection.png"), 14);
    tap_key(&mut app, "sel-search");
    assert!(app.find_rect("sel-search-book").is_some());
    assert!(app.find_rect("sel-dict").is_some());
}

#[test]
fn holding_a_word_reports_encrypted_kobo_dictionaries_and_offers_free_downloads() {
    use std::io::Write;
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("nickel-encrypted-dictionary");
    let mut app = start(&dir, "juliet.epub");
    let folder = app.services().paths.books_dir.join(".kobo/dict");
    std::fs::create_dir_all(&folder).unwrap();
    let mut zip = zip::ZipWriter::new(std::fs::File::create(folder.join("dicthtml-en.zip")).unwrap());
    zip.start_file("re.html", zip::write::SimpleFileOptions::default())
        .unwrap();
    zip.write_all(&[0xa5; 32]).unwrap();
    zip.finish().unwrap();
    app.command("reader:location", &[Value::Number(5.0)]);
    wait_reader(&mut app);
    app.command("reader:hold", &[Value::Number(800.0), Value::Number(830.0)]);
    wait_reader(&mut app);
    app.command("reader:release", &[]);
    wait_reader(&mut app);
    let mut text = String::new();
    app.update_top::<kaia_ui::components::Paragraph, _>("dict-body", |p, _| text = p.text().into());
    assert!(text.contains("encrypted"), "{text}");
    assert!(text.contains("free dictionary downloads"), "{text}");
    assert!(app.find_rect("dict-downloads").is_some());
}
