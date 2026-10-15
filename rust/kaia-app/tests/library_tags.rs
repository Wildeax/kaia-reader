//! Favorites and tags in the Library: the star and the Tags row of the book
//! details, the tag sheet that adds and removes tags, and the Favorites and Tags
//! choices of the filter. The marks live in the library index, so they survive a
//! restart.

mod common;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use kaia_app::{App, HeadlessBackend, Route};
use kaia_display::InputEvent;
use kaia_gfx::golden::assert_matches_golden;
use kaia_input::{Key, Phase, TouchEvent};
use kaia_library::Tag;

use common::*;

/// The key a button with `action` has in the tree: `library:sort` is `library-sort`.
fn key(action: &str) -> String {
    action.replace(':', "-")
}

/// A library app with the three test books indexed and Home on screen.
fn indexed(name: &str) -> (App<HeadlessBackend>, PathBuf) {
    let dir = scratch(name);
    let lib = library_with_books(&dir);
    let mut app = make_app(&dir, vec![lib]);
    app.start().unwrap();
    wait_for(&mut app, "the library scan", |a| !a.scan_running());
    app.iterate(Some(Duration::from_millis(10))).unwrap();
    (app, dir)
}

/// Shuts `app` down and starts a new one on the same data folder, with no scan.
fn restart(mut app: App<HeadlessBackend>, dir: &Path) -> App<HeadlessBackend> {
    app.shutdown();
    drop(app);
    let mut app = make_app(dir, Vec::new());
    app.start().unwrap();
    app
}

/// Opens the Library and shows the Books tab as a list, so every book has a row.
fn open_library(app: &mut App<HeadlessBackend>) {
    if app.top_route() != Some(&Route::Library { page: 0 }) {
        tap_key(app, "library");
    }
    assert_eq!(app.top_route(), Some(&Route::Library { page: 0 }));
    tap_key(app, "library-view-list");
}

/// Holds a finger on `widget` until the details sheet is up, then lifts it.
fn hold_for_details(app: &mut App<HeadlessBackend>, widget: &str) {
    let p = centre_of(app, widget);
    app.process(vec![InputEvent::Touch(TouchEvent {
        slot: 0,
        phase: Phase::Down,
        pos: p,
        time_us: 0,
    })])
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while app.find_rect(&key("library:panel:close")).is_none() {
        assert!(Instant::now() < deadline, "the details sheet never showed");
        app.iterate(Some(Duration::from_millis(20))).unwrap();
    }
    app.process(vec![InputEvent::Touch(TouchEvent {
        slot: 0,
        phase: Phase::Up,
        pos: p,
        time_us: 0,
    })])
    .unwrap();
}

/// Types `text` into the focused field, one key at a time.
fn type_text(app: &mut App<HeadlessBackend>, text: &str) {
    for c in text.chars() {
        app.key(Key::Char(c)).unwrap();
    }
}

/// The Books tab shows the book with `id`.
fn shows(app: &App<HeadlessBackend>, id: i64) -> bool {
    app.find_rect(&format!("book-{id}")).is_some()
}

/// Opens the typing of a new tag on the tag sheet, types `text` and adds it.
fn add_typed(app: &mut App<HeadlessBackend>, book: i64, text: &str) {
    tap_key(app, &key(&format!("library:tags:new:{book}")));
    tap_key(app, "tag-field");
    type_text(app, text);
    tap_key(app, &key(&format!("library:tag:add:{book}")));
}

/// The names of the tags of a book, in the order the library gives them.
fn tag_names(app: &App<HeadlessBackend>, book: i64) -> Vec<String> {
    library_db(app)
        .book_tags(book)
        .unwrap()
        .into_iter()
        .map(|t| t.name)
        .collect()
}

/// The id of the tag `name` on a book.
fn tag_id(app: &App<HeadlessBackend>, book: i64, name: &str) -> i64 {
    library_db(app)
        .book_tags(book)
        .unwrap()
        .into_iter()
        .find(|t: &Tag| t.name == name)
        .unwrap_or_else(|| panic!("the book has no tag {name:?}"))
        .id
}

#[test]
fn a_favorite_is_set_from_the_details_and_the_filter_shows_it() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, _dir) = indexed("favorite-filter");
    let juliet = book_id(&app, "juliet.epub");
    let leaves = book_id(&app, "leaves.epub");
    open_library(&mut app);

    hold_for_details(&mut app, &format!("book-{juliet}"));
    let on = key(&format!("library:favorite:{juliet}:on"));
    assert!(app.find_rect(&on).is_some(), "an unmarked book offers to mark it");
    tap_key(&mut app, &on);
    assert!(library_db(&app).is_favorite(juliet).unwrap());
    assert!(
        app.find_rect(&key(&format!("library:favorite:{juliet}:off")))
            .is_some(),
        "the star now offers to unmark the book"
    );
    tap_key(&mut app, &key("library:panel:close"));

    tap_key(&mut app, "library-filter");
    tap_key(&mut app, "library-filter-favorite");
    assert!(shows(&app, juliet), "the favorites filter keeps the favorite");
    assert!(!shows(&app, leaves), "the favorites filter hides the other book");
    assert_eq!(
        app.services().settings.read("library.favorite").as_bool(),
        Some(true)
    );

    tap_key(&mut app, "library-filter");
    tap_key(&mut app, "library-filter-favorite");
    assert!(shows(&app, leaves), "a second tap shows every book again");
    assert_eq!(
        app.services().settings.read("library.favorite").as_bool(),
        Some(false)
    );
}

#[test]
fn tags_are_added_from_the_sheet_once_and_removed() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, _dir) = indexed("tags-sheet");
    let juliet = book_id(&app, "juliet.epub");
    open_library(&mut app);
    hold_for_details(&mut app, &format!("book-{juliet}"));

    tap_key(&mut app, &key(&format!("library:tags:open:{juliet}")));
    assert!(
        app.find_rect("tag-field").is_none(),
        "the sheet opens on the list"
    );
    assert!(app
        .find_rect(&key(&format!("library:tags:new:{juliet}")))
        .is_some());
    add_typed(&mut app, juliet, "Sci-Fi");
    assert_eq!(tag_names(&app, juliet), vec!["Sci-Fi"]);
    assert!(
        app.find_rect("tag-field").is_none(),
        "a tag added returns to the list"
    );

    // The same tag in other case, with spaces around it, is not added again.
    add_typed(&mut app, juliet, "  sci-fi  ");
    assert_eq!(tag_names(&app, juliet), vec!["Sci-Fi"], "no second copy");

    add_typed(&mut app, juliet, "classic");
    assert_eq!(tag_names(&app, juliet), vec!["classic", "Sci-Fi"]);

    let sci = tag_id(&app, juliet, "Sci-Fi");
    tap_key(&mut app, &key(&format!("library:tag:remove:{juliet}:{sci}")));
    assert_eq!(
        tag_names(&app, juliet),
        vec!["classic"],
        "Remove takes the tag off"
    );

    tap_key(&mut app, &key(&format!("library:tags:back:{juliet}")));
    assert!(
        app.find_rect(&key(&format!("library:tags:open:{juliet}")))
            .is_some(),
        "Back returns to the details"
    );
    assert!(
        app.find_rect("details-tags").is_some(),
        "the details show the tags"
    );
}

#[test]
fn the_tag_filter_lists_the_tags_and_survives_a_restart() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, dir) = indexed("tag-filter");
    let juliet = book_id(&app, "juliet.epub");
    let leaves = book_id(&app, "leaves.epub");
    app.services_mut().library.add_tag(juliet, "classic").unwrap();
    app.services_mut().library.add_tag(leaves, "classic").unwrap();
    app.services_mut().library.add_tag(leaves, "war").unwrap();
    let war = tag_id(&app, leaves, "war");
    open_library(&mut app);

    tap_key(&mut app, "library-filter");
    tap_key(&mut app, "library-filter-tags");
    assert!(
        app.find_rect(&key("library:filter:tag:none")).is_none(),
        "no tag filter means no Show all tags button"
    );
    tap_key(&mut app, &key(&format!("library:filter:tag:{war}")));
    assert!(!shows(&app, juliet), "the war tag hides the book without it");
    assert!(shows(&app, leaves), "the war tag keeps its book");
    assert_eq!(app.services().settings.read("library.tag").as_str(), Some("war"));

    let mut app = restart(app, &dir);
    open_library(&mut app);
    assert!(
        !shows(&app, juliet) && shows(&app, leaves),
        "the tag filter comes back"
    );

    tap_key(&mut app, "library-filter");
    tap_key(&mut app, "library-filter-tags");
    tap_key(&mut app, &key("library:filter:tag:none"));
    assert!(
        shows(&app, juliet) && shows(&app, leaves),
        "Show all tags clears the filter"
    );
    assert_eq!(app.services().settings.read("library.tag").as_str(), Some(""));
}

#[test]
fn favorites_and_tags_survive_a_restart_and_show_in_the_details() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, dir) = indexed("restart");
    let juliet = book_id(&app, "juliet.epub");
    app.services_mut().library.set_favorite(juliet, true).unwrap();
    app.services_mut().library.add_tag(juliet, "Sci-Fi").unwrap();
    app.services_mut().library.add_tag(juliet, "classic").unwrap();

    let mut app = restart(app, &dir);
    open_library(&mut app);
    hold_for_details(&mut app, &format!("book-{juliet}"));
    assert!(
        app.find_rect(&key(&format!("library:favorite:{juliet}:off")))
            .is_some(),
        "the favorite star is on after the restart"
    );
    assert!(
        app.find_rect("details-tags").is_some(),
        "the tags show in the details"
    );
    assert_eq!(tag_names(&app, juliet), vec!["classic", "Sci-Fi"]);
}

#[test]
fn the_tag_limit_refuses_the_twenty_first_tag_with_a_message() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, _dir) = indexed("tag-limit");
    let juliet = book_id(&app, "juliet.epub");
    for n in 0..kaia_library::MAX_TAGS_PER_BOOK {
        app.services_mut()
            .library
            .add_tag(juliet, &format!("tag {n}"))
            .unwrap();
    }
    open_library(&mut app);
    hold_for_details(&mut app, &format!("book-{juliet}"));
    tap_key(&mut app, &key(&format!("library:tags:open:{juliet}")));
    tap_key(&mut app, &key(&format!("library:tags:new:{juliet}")));
    tap_key(&mut app, "tag-field");
    type_text(&mut app, "one more");
    tap_key(&mut app, &key(&format!("library:tag:add:{juliet}")));
    assert_eq!(
        tag_names(&app, juliet).len(),
        kaia_library::MAX_TAGS_PER_BOOK,
        "the twenty-first tag is not stored"
    );
    assert!(
        app.find_rect("tag-field").is_some(),
        "the sheet stays open and keeps the text for editing"
    );
}

#[test]
fn details_golden_with_a_favorite_and_tags() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, _dir) = indexed("golden-details-tags");
    let juliet = book_id(&app, "juliet.epub");
    app.services_mut().library.set_favorite(juliet, true).unwrap();
    app.services_mut().library.add_tag(juliet, "Sci-Fi").unwrap();
    app.services_mut().library.add_tag(juliet, "classic").unwrap();
    open_library(&mut app);
    hold_for_details(&mut app, &format!("book-{juliet}"));
    wait_for(&mut app, "visible covers", |_| !kaia_app::covers::busy());
    assert_matches_golden(app.frame(), &golden("library_details_tags.png"), 14);
}

#[test]
fn tag_filter_golden() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, _dir) = indexed("golden-tag-filter");
    let juliet = book_id(&app, "juliet.epub");
    let leaves = book_id(&app, "leaves.epub");
    app.services_mut().library.add_tag(juliet, "classic").unwrap();
    app.services_mut().library.add_tag(leaves, "classic").unwrap();
    app.services_mut().library.add_tag(leaves, "war").unwrap();
    open_library(&mut app);
    tap_key(&mut app, "library-filter");
    tap_key(&mut app, "library-filter-tags");
    assert_matches_golden(app.frame(), &golden("library_tag_filter.png"), 14);
}

#[test]
fn tag_sheet_golden() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, _dir) = indexed("golden-tag-sheet");
    let juliet = book_id(&app, "juliet.epub");
    app.services_mut().library.add_tag(juliet, "Sci-Fi").unwrap();
    app.services_mut().library.add_tag(juliet, "classic").unwrap();
    open_library(&mut app);
    hold_for_details(&mut app, &format!("book-{juliet}"));
    tap_key(&mut app, &key(&format!("library:tags:open:{juliet}")));
    assert_matches_golden(app.frame(), &golden("library_tags_list.png"), 14);
    tap_key(&mut app, &key(&format!("library:tags:new:{juliet}")));
    tap_key(&mut app, "tag-field");
    type_text(&mut app, "Sci");
    assert_matches_golden(app.frame(), &golden("library_tags_sheet.png"), 14);
}
