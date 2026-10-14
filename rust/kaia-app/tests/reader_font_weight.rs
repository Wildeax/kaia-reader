//! The text weight of a reflowable book. The weight choice is saved for the font
//! family in settings, the book's layout keeps it, and a reopened book shows it.

mod common;

use common::*;
use kaia_app::{App, HeadlessBackend};
use kaia_events::{Event, Settings, Value};

/// Waits until the reader and the library scan are idle.
fn settle(app: &mut App<HeadlessBackend>) {
    wait_for(app, "the reader to settle", |a| {
        !a.reader_busy() && !a.scan_running()
    });
    app.process(vec![]).unwrap();
}

fn open(app: &mut App<HeadlessBackend>, name: &str) {
    let id = book_id(app, name);
    app.command(&format!("open_book:{id}"), &[]);
    settle(app);
}

/// The weight saved in the book's state, after the reader saves it.
fn saved_weight(app: &mut App<HeadlessBackend>, name: &str) -> f64 {
    app.broadcast_app(Event::new("SaveState"));
    let id = book_id(app, name);
    let book = app.services().library.book(id).unwrap().unwrap();
    let path = app
        .services()
        .paths
        .data_dir
        .join("kaia/books")
        .join(format!("{}.json", book.partial_md5));
    Settings::open(&path)
        .read("layout")
        .get("font_weight")
        .as_f64()
        .expect("a saved font weight")
}

fn start(dir: &std::path::Path, books: &std::path::Path) -> App<HeadlessBackend> {
    let mut app = make_app(dir, vec![books.to_path_buf()]);
    app.start().unwrap();
    wait_for(&mut app, "the library scan", |a| !a.scan_running());
    app
}

#[test]
fn weight_is_saved_per_font_family_and_comes_back_on_reopen() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("font-weight");
    let books = library_with_books(&dir);
    let mut app = start(&dir, &books);
    open(&mut app, "juliet.epub");
    // Picking a face pops the font list, which returns to the sheet. The sheet must be open.
    app.command("reader:panel:layout", &[]);
    settle(&mut app);
    assert_eq!(saved_weight(&mut app, "juliet.epub"), 0.0, "text starts regular");

    // Index 4 of the weight choices is +1, which is 500 in CREngine.
    app.command("reader:layout:weight", &[Value::Number(4.0)]);
    settle(&mut app);
    assert_eq!(saved_weight(&mut app, "juliet.epub"), 1.0);

    // Noto Serif has no saved weight, so it starts regular. Then it gets -1.
    app.command("reader:layout:font:1", &[]);
    settle(&mut app);
    assert_eq!(
        saved_weight(&mut app, "juliet.epub"),
        0.0,
        "a face with no saved weight is regular"
    );
    app.command("reader:layout:weight", &[Value::Number(0.0)]);
    settle(&mut app);
    assert_eq!(saved_weight(&mut app, "juliet.epub"), -1.0);

    // Back to the book's own font, which still has +1.
    app.command("reader:layout:font:0", &[]);
    settle(&mut app);
    assert_eq!(
        saved_weight(&mut app, "juliet.epub"),
        1.0,
        "the book's font keeps its own weight"
    );

    let settings_path = dir.join("data/settings.json");
    let saved = Settings::read_exact(&settings_path)
        .unwrap()
        .expect("settings.json");
    assert_eq!(saved.read("reading.font_weight.(book)").as_f64(), Some(1.0));
    assert_eq!(saved.read("reading.font_weight.Noto Serif").as_f64(), Some(-1.0));
    assert!(app.unknown_commands().is_empty());
    drop(app);

    // A new run: the reopened book keeps +1. A new book in Noto Serif starts at -1.
    let mut settings = Settings::open(&settings_path);
    settings.save("reading.font_family", Value::from("Noto Serif"));
    settings.flush(&settings_path).unwrap();
    let mut app = start(&dir, &books);
    open(&mut app, "juliet.epub");
    assert_eq!(
        saved_weight(&mut app, "juliet.epub"),
        1.0,
        "the reopened book keeps +1"
    );
    open(&mut app, "leaves.epub");
    assert_eq!(
        saved_weight(&mut app, "leaves.epub"),
        -1.0,
        "a new book takes its family's weight"
    );
}
