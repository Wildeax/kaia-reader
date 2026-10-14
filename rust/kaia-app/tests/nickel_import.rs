//! Synthetic Nickel data exercises the full plan and staged import.

mod common;

use kaia_app::nickel::{Counts, Plan};
use kaia_events::{Settings, Value};
use kaia_library::{Filter, Library, Sort, SortKey, Status};
use rusqlite::{params, Connection};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

struct Fixture {
    _dir: tempfile::TempDir,
    books: PathBuf,
    data: PathBuf,
    library: Library,
    source: Connection,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let books = common::library_with_books(dir.path());
        let data = dir.path().join("data");
        fs::create_dir_all(&data).unwrap();
        let mut library = Library::open(&data.join("library.sqlite3")).unwrap();
        library
            .scan(std::slice::from_ref(&books), |_| {}, &AtomicBool::new(false))
            .unwrap();
        fs::create_dir_all(books.join(".kobo")).unwrap();
        let source = Connection::open(books.join(".kobo/KoboReader.sqlite")).unwrap();
        source.execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0;
            CREATE TABLE DbVersion (version INTEGER); INSERT INTO DbVersion VALUES (176);
            CREATE TABLE content (ContentID TEXT, ContentType INTEGER, BookID TEXT, adobe_location TEXT,
                Title TEXT, Attribution TEXT, ___PercentRead REAL, ReadStatus INTEGER,
                ChapterIDBookmarked TEXT, ParagraphBookmarked INTEGER, BookmarkWordOffset INTEGER);
            CREATE TABLE Bookmark (BookmarkID TEXT, VolumeID TEXT, Text TEXT, Annotation TEXT, DateCreated TEXT, _IsDeleted TEXT);
            CREATE TABLE Shelf (Id TEXT, Name TEXT, _IsDeleted TEXT);
            CREATE TABLE ShelfContent (ShelfName TEXT, ContentId TEXT, _IsDeleted TEXT);
            CREATE TABLE WordList (Text TEXT, VolumeId TEXT, DictSuffix TEXT, DateCreated TEXT);
            CREATE TABLE Ratings (ContentID TEXT, Rating INTEGER, Review TEXT);
            CREATE TABLE KoboPlusReadingSessions (ContentId TEXT, StartTime TEXT, EndTime TEXT, StartPercentRead REAL, EndPercentRead REAL, IdleTime INTEGER);
            CREATE TABLE SecureUser (secret TEXT); INSERT INTO SecureUser VALUES ('must never be read');
            CREATE TABLE AnalyticsEvents (payload TEXT); INSERT INTO AnalyticsEvents VALUES ('must never be read');
            INSERT INTO content VALUES ('path-book',6,NULL,'file:///mnt/onboard/juliet.epub','wrong title','wrong author',42,1,'secret-position',99,88);
            INSERT INTO content VALUES ('missing-book',6,NULL,'missing.epub','absent','absent',99,2,NULL,NULL,NULL);
            INSERT INTO Bookmark VALUES ('found','path-book','Romeo','Read this again','2026-01-01T00:00:00Z','false');
            INSERT INTO Bookmark VALUES ('lost','path-book','this passage does not occur anywhere in the fixture','missing note','2026-01-01T00:00:00Z','false');
            INSERT INTO Bookmark VALUES ('empty','path-book','','unanchored note','2026-01-01T00:00:00Z','false');
            INSERT INTO Shelf VALUES ('1','Keep','false'),('2','Deleted','true');
            INSERT INTO ShelfContent VALUES ('Keep','path-book','false'),('Keep','title-book','false'),('Deleted','path-book','false'),('Keep','missing-book','false');
            INSERT INTO WordList VALUES ('Romeo','path-book','-en','2026-01-01T00:00:00Z');
            INSERT INTO Ratings VALUES ('path-book',4,'My review');
            INSERT INTO KoboPlusReadingSessions VALUES ('path-book','2026-01-01T00:00:00Z','2026-01-01T00:10:00Z',10,20,60);
        ").unwrap();
        let title_book = library.book_by_path(&books.join("leaves.epub")).unwrap().unwrap();
        source.execute("INSERT INTO content (ContentID,ContentType,adobe_location,Title,Attribution,___PercentRead,ReadStatus)
            VALUES ('title-book',6,'lost.epub',?1,?2,100,2)", params![title_book.title, title_book.authors]).unwrap();
        Self {
            _dir: dir,
            books,
            data,
            library,
            source,
        }
    }
    fn plan(&self) -> Plan {
        Plan::prepare(&self.books, &self.library).unwrap()
    }
    fn state(&self, file: &str) -> Value {
        let hash = kaia_book::partial_md5(&self.books.join(file)).unwrap();
        let state = Settings::read_exact(&self.data.join(format!("kaia/books/{hash}.json")))
            .unwrap()
            .unwrap();
        Value::Table(state.table().clone())
    }
}

fn source_files(root: &Path) -> Vec<(PathBuf, Vec<u8>, std::time::SystemTime)> {
    let mut files: Vec<_> = fs::read_dir(root.join(".kobo"))
        .unwrap()
        .map(|entry| {
            let path = entry.unwrap().path();
            let metadata = fs::metadata(&path).unwrap();
            (
                path.clone(),
                fs::read(path).unwrap(),
                metadata.modified().unwrap(),
            )
        })
        .collect();
    files.sort_by(|a, b| a.0.cmp(&b.0));
    files
}

#[test]
fn plan_import_and_repeat_preserve_source_including_live_wal() {
    let _serial = common::SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let mut f = Fixture::new();
    let before = source_files(&f.books);
    assert!(before
        .iter()
        .any(|(p, bytes, _)| p.to_string_lossy().ends_with("-wal") && !bytes.is_empty()));
    let plan = f.plan();
    assert_eq!(plan.version, 176);
    assert_eq!(plan.tables.len(), 8);
    assert!(!plan.tables.contains_key("SecureUser"));
    assert!(!plan.tables.contains_key("AnalyticsEvents"));
    assert_eq!(plan.tables["content"], 3);
    assert_eq!(
        plan.counts,
        Counts {
            books: 2,
            highlights: 3,
            notes: 3,
            collections: 2,
            words: 1,
            ratings: 1,
            sessions: 1
        }
    );
    assert!(!f.data.join("kaia").exists(), "planning writes no owner data");
    let report = plan.import(&f.data, &mut f.library).unwrap();
    assert_eq!(
        report.imported,
        Counts {
            books: 2,
            highlights: 1,
            notes: 1,
            collections: 1,
            words: 1,
            ratings: 1,
            sessions: 1
        }
    );
    assert_eq!(report.skipped.books, 1);
    assert_eq!(report.skipped.highlights, 2);
    let state = f.state("juliet.epub");
    assert_eq!(state.get("percent_finished").as_f64(), Some(0.42));
    assert_eq!(state.get("summary").get("status").as_str(), Some("reading"));
    assert_eq!(state.get("summary").get("rating").as_i64(), Some(4));
    assert_eq!(state.get("summary").get("note").as_str(), Some("My review"));
    assert!(state.get("last_xpointer").is_nil());
    assert!(state.get("last_page").is_nil());
    assert_eq!(state.get("nickel_reading_seconds").as_f64(), Some(540.0));
    assert_eq!(state.get("reading_time").array_len(), 0);
    let Value::Table(annotations) = state.get("annotations") else {
        panic!("annotations")
    };
    let highlight = annotations.values().next().unwrap();
    assert_eq!(highlight.get("text").as_str(), Some("Romeo"));
    assert_eq!(highlight.get("note").as_str(), Some("Read this again"));
    assert!(!highlight.get("pos0").is_nil());
    assert!(!highlight.get("pos1").is_nil());
    let mut options = kaia_book::OpenOptions::new(&f.data);
    options.import_only = true;
    let mut session = kaia_book::BookSession::open(f.books.join("juliet.epub"), options).unwrap();
    let annotation = session.annotations().unwrap().pop().unwrap();
    assert!(session
        .annotation_text(annotation.id)
        .unwrap()
        .to_lowercase()
        .contains("romeo"));
    drop(session);
    assert_eq!(
        f.state("leaves.epub").get("summary").get("status").as_str(),
        Some("complete")
    );
    assert_eq!(
        f.library
            .book_by_path(&f.books.join("leaves.epub"))
            .unwrap()
            .unwrap()
            .status,
        Status::Complete
    );
    let collections = f.library.collections().unwrap();
    assert_eq!(collections.len(), 1);
    assert_eq!(collections[0].books, 2);
    let store = kaia_storage::Store::open(&f.data.join("kaia")).unwrap();
    let words = store.list_words().unwrap();
    assert_eq!(words.len(), 1);
    assert_eq!(words[0].dict_suffix.as_deref(), Some("-en"));
    assert!(words[0].book_title.is_some());
    drop(store);
    let mut stats = kaia_stats::Stats::open(f.data.join("statistics.sqlite3")).unwrap();
    assert_eq!(stats.books().unwrap()[0].seconds, 540);
    assert_eq!(stats.books().unwrap()[0].distinct_pages, 0);
    assert_eq!(stats.daily(1767226200, 1).unwrap()[0].seconds, 540);
    drop(stats);
    let state_before = fs::read(f.data.join(format!(
        "kaia/books/{}.json",
        kaia_book::partial_md5(&f.books.join("juliet.epub")).unwrap()
    )))
    .unwrap();
    let report = f.plan().import(&f.data, &mut f.library).unwrap();
    assert_eq!(report.imported.total(), 0);
    assert_eq!(f.library.collections().unwrap()[0].books, 2);
    assert_eq!(
        fs::read(f.data.join(format!(
            "kaia/books/{}.json",
            kaia_book::partial_md5(&f.books.join("juliet.epub")).unwrap()
        )))
        .unwrap(),
        state_before
    );
    assert_eq!(source_files(&f.books), before);
}

#[test]
fn secret_view_is_refused_and_absent_optional_tables_are_supported() {
    let _serial = common::SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let mut f = Fixture::new();
    f.source
        .execute_batch("DROP TABLE WordList; CREATE VIEW WordList AS SELECT secret AS Text FROM SecureUser;")
        .unwrap();
    assert!(Plan::prepare(&f.books, &f.library).is_err());
    f.source.execute_batch("DROP VIEW WordList; DROP TABLE Bookmark; DROP TABLE Shelf; DROP TABLE ShelfContent; DROP TABLE Ratings; DROP TABLE KoboPlusReadingSessions;").unwrap();
    let plan = f.plan();
    assert_eq!(plan.counts.words, 0);
    assert_eq!(plan.counts.highlights, 0);
    let report = plan.import(&f.data, &mut f.library).unwrap();
    assert_eq!(report.imported.books, 2);
}

#[test]
fn suffix_is_optional_and_existing_words_and_book_edits_win_on_repeat() {
    let _serial = common::SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let mut f = Fixture::new();
    f.source
        .execute_batch("ALTER TABLE WordList DROP COLUMN DictSuffix;")
        .unwrap();
    f.plan().import(&f.data, &mut f.library).unwrap();
    let mut store = kaia_storage::Store::open(&f.data.join("kaia")).unwrap();
    assert_eq!(store.list_words().unwrap()[0].dict_suffix, None);
    store
        .add_word("ROMEO", Some("My book"), Some("My context"), 123)
        .unwrap();
    drop(store);
    let mut session =
        kaia_book::BookSession::open(f.books.join("juliet.epub"), kaia_book::OpenOptions::new(&f.data))
            .unwrap();
    session.set_rating(2).unwrap();
    session.delete_annotation(1).unwrap();
    drop(session);
    assert_eq!(
        f.plan().import(&f.data, &mut f.library).unwrap().imported.total(),
        0
    );
    assert_eq!(
        f.state("juliet.epub").get("summary").get("rating").as_i64(),
        Some(2)
    );
    assert_eq!(f.state("juliet.epub").get("annotations").array_len(), 0);
    assert_eq!(
        kaia_storage::Store::open(&f.data.join("kaia"))
            .unwrap()
            .list_words()
            .unwrap()[0]
            .context
            .as_deref(),
        Some("My context")
    );
}

#[test]
fn ambiguous_title_matches_are_skipped() {
    let _serial = common::SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let mut f = Fixture::new();
    fs::copy(f.books.join("leaves.epub"), f.books.join("another.epub")).unwrap();
    f.library
        .scan(std::slice::from_ref(&f.books), |_| {}, &AtomicBool::new(false))
        .unwrap();
    assert_eq!(f.plan().counts.books, 1);
    let books = f
        .library
        .books(Sort::new(SortKey::Title, false), &Filter::default(), 0, 100)
        .unwrap();
    assert_eq!(books.len(), 4);
}

#[test]
fn settings_shows_import_only_with_source_and_waits_for_confirmation() {
    let _serial = common::SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let f = Fixture::new();
    let mut app = common::make_app(f._dir.path(), Vec::new());
    app.services_mut().paths.books_dir = f.books.clone();
    app.start().unwrap();
    use kaia_app::surfaces::settings::{entries::Entry, sections};
    let has_import = |app: &kaia_app::App<kaia_app::HeadlessBackend>| {
        sections::entries("data", app.services(), 0)
            .iter()
            .any(|e| matches!(e,Entry::Button { action,.. } if action == "settings:data:kobo"))
    };
    assert!(has_import(&app));
    app.services_mut().paths.books_dir = f._dir.path().join("absent");
    assert!(!has_import(&app));
    app.services_mut().paths.books_dir = f.books.clone();
    app.command("nav:settings", &[]);
    app.command("settings:data", &[]);
    app.command("settings:data:kobo", &[]);
    app.process(Vec::new()).unwrap();
    kaia_gfx::golden::assert_matches_golden(app.frame(), &common::golden("settings_kobo_plan.png"), 14);
    assert!(!f.data.join("kaia/books").exists());
    app.command("settings:data:kobo:confirm", &[]);
    assert!(!f.data.join("kaia/books").exists());
    app.process(Vec::new()).unwrap();
    common::tap_key(&mut app, "dialog-button:settings:data:kobo:cancel");
    app.command("settings:data:kobo:close", &[]);
    assert!(!f.data.join("kaia/books").exists());
    app.command("settings:data:kobo", &[]);
    app.command("settings:data:kobo:confirm", &[]);
    app.process(Vec::new()).unwrap();
    common::tap_key(&mut app, "dialog-button:settings:data:kobo:run");
    assert!(app.notices().is_empty(), "{:?}", app.notices());
    kaia_gfx::golden::assert_matches_golden(app.frame(), &common::golden("settings_kobo_report.png"), 14);
    assert_eq!(
        f.state("juliet.epub").get("percent_finished").as_f64(),
        Some(0.42)
    );
}

#[test]
fn an_open_book_or_scan_prevents_replacement() {
    let _serial = common::SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let mut f = Fixture::new();
    let session =
        kaia_book::BookSession::open(f.books.join("juliet.epub"), kaia_book::OpenOptions::new(&f.data))
            .unwrap();
    assert!(f.plan().import(&f.data, &mut f.library).is_err());
    assert!(!f.data.join("kaia/kaia.sqlite3").exists());
    drop(session);
    let lock = fs::File::options()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(f.data.join("nickel-import.lock"))
        .unwrap();
    lock.lock().unwrap();
    assert!(f.plan().import(&f.data, &mut f.library).is_err());
    assert!(!f.data.join("kaia/kaia.sqlite3").exists());
}

#[test]
fn invalid_sessions_and_deleted_members_are_skipped() {
    let _serial = common::SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let mut f = Fixture::new();
    f.source
        .execute_batch(
            "UPDATE ShelfContent SET _IsDeleted='true' WHERE ContentId='title-book';
        UPDATE KoboPlusReadingSessions SET StartTime='2026-01-01T00:00:00.000', EndTime='2026-01-01 00:10:00.000';
        INSERT INTO KoboPlusReadingSessions VALUES ('path-book','bad','bad',0,0,0);
        INSERT INTO KoboPlusReadingSessions VALUES ('path-book','100','50',0,20,0);
        INSERT INTO KoboPlusReadingSessions VALUES ('path-book','100','200',0,120,0);
        INSERT INTO KoboPlusReadingSessions VALUES ('missing-book','100','200',0,20,0);",
        )
        .unwrap();
    let report = f.plan().import(&f.data, &mut f.library).unwrap();
    assert_eq!(report.imported.sessions, 1);
    assert_eq!(report.skipped.sessions, 4);
    assert_eq!(f.library.collections().unwrap()[0].books, 1);
}

#[cfg(unix)]
#[test]
fn target_symlinks_cannot_write_inside_kobo() {
    let _serial = common::SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let mut f = Fixture::new();
    std::os::unix::fs::symlink(f.books.join(".kobo"), f.data.join("kaia")).unwrap();
    let before = source_files(&f.books);
    assert!(f.plan().import(&f.data, &mut f.library).is_err());
    assert_eq!(source_files(&f.books), before);
}

#[test]
fn confirmation_uses_the_planned_snapshot_and_preserves_an_existing_position() {
    let _serial = common::SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let mut f = Fixture::new();
    let path = f.books.join("juliet.epub");
    let mut session = kaia_book::BookSession::open(&path, kaia_book::OpenOptions::new(&f.data)).unwrap();
    session.go_to_percent(0.2).unwrap();
    session.save().unwrap();
    drop(session);
    let position = f.state("juliet.epub").get("last_xpointer").clone();
    let plan = f.plan();
    f.source
        .execute("UPDATE WordList SET Text='changed after plan'", [])
        .unwrap();
    let report = plan.import(&f.data, &mut f.library).unwrap();
    assert_eq!(report.imported.words, 1);
    assert_eq!(f.state("juliet.epub").get("last_xpointer"), &position);
    let words = kaia_storage::Store::open(&f.data.join("kaia"))
        .unwrap()
        .list_words()
        .unwrap();
    assert_eq!(words[0].word, "Romeo");
}

#[test]
fn staging_failure_leaves_existing_owners_unchanged() {
    let _serial = common::SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let mut f = Fixture::new();
    fs::create_dir_all(f.data.join("kaia")).unwrap();
    let target = f.data.join("kaia/kaia.sqlite3");
    Connection::open(&target)
        .unwrap()
        .execute_batch("PRAGMA user_version=999")
        .unwrap();
    let before = fs::read(&target).unwrap();
    let library_before = fs::read(f.data.join("library.sqlite3")).unwrap();
    assert!(f.plan().import(&f.data, &mut f.library).is_err());
    assert_eq!(fs::read(&target).unwrap(), before);
    assert_eq!(fs::read(f.data.join("library.sqlite3")).unwrap(), library_before);
    assert!(!f.data.join("kaia/books").exists());
    assert!(!f.data.join("statistics.sqlite3").exists());
}
