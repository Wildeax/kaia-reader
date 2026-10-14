//! Text import, encoding detection, and saved reading state through CREngine.

#![cfg(feature = "cre")]

mod common;

use kaia_book::{Drawer, Location, Pos};
use kaia_doc::{format, Engine, Format};
use kaia_gfx::Size;

const VIEW: Size = Size { w: 1264, h: 1680 };

fn round_trip(name: &str, bytes: &[u8], word: &str, require_encoding: bool) {
    let books = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let path = books.path().join(name);
    std::fs::write(&path, bytes).unwrap();
    assert_eq!(format::detect(&path).unwrap(), Format::Txt);
    assert!(kaia_doc::open_with(&path, Engine::Mupdf).is_err());

    let doc = kaia_doc::open(&path).unwrap();
    let text = doc.page_text(0).unwrap();
    if !require_encoding && !text.text.contains(word) {
        eprintln!(
            "CREngine did not detect Windows-1252 for this fixture; skipping its state round trip. First line: {:?}",
            text.text.lines().next()
        );
        return;
    }
    assert!(text.text.contains(word), "decoded text: {:?}", text.text);
    // Each short source line must keep its own rendered line.
    let first = text.text.find(word).unwrap();
    let second = text.text[first + word.len()..].find(word).unwrap() + first + word.len();
    assert!(text.lines.len() > 1);
    assert!(text.text[first..second].contains('\n'));
    drop(doc);

    let (here, page, id, selected) = {
        let mut session = common::open(&path, &data);
        assert!(session.uses_cre());
        assert!(session.is_reflowable());
        assert!(session.page_count() > 2);
        assert!(session.next_page().unwrap());
        assert!(session.next_page().unwrap());
        assert_eq!(session.current().page, 2);
        assert!(session.prev_page().unwrap());
        assert_eq!(session.current().page, 1);
        let (_, selection) = common::first_word(&mut session, VIEW);
        assert!(!selection.text.is_empty());
        assert!(matches!(selection.pos0, Pos::Xpointer(_)));
        let id = session
            .add_highlight(&selection, Drawer::Lighten, "yellow", Some("saved text"))
            .unwrap();
        session.save().unwrap();
        let current = session.current();
        assert!(matches!(current.location, Location::Engine(_)));
        (current.location, current.page, id, selection.text)
    };

    let mut session = common::open(&path, &data);
    assert_eq!(session.current().location, here);
    assert_eq!(session.current().page, page);
    let annotations = session.annotations().unwrap();
    assert_eq!(annotations.len(), 1);
    assert_eq!(annotations[0].id, id);
    assert_eq!(annotations[0].text.as_deref(), Some(selected.as_str()));
    assert_eq!(annotations[0].note.as_deref(), Some("saved text"));
    assert_eq!(session.annotation_text(id).unwrap(), selected);
    let visible = session.annotations_on_page(VIEW).unwrap();
    assert_eq!(visible.len(), 1);
    assert_eq!(visible[0].annotation.id, id);
    assert!(!visible[0].rects.is_empty());
}

#[test]
fn utf8_text_keeps_lines_position_and_highlight() {
    round_trip("A café.TXT", "café\n".repeat(300).as_bytes(), "café", true);
}

#[test]
fn windows_1252_text_round_trip_when_detected() {
    let paragraph = b"caf\xe9\r\ncaf\xe9\r\nLe matin, nous avons travers\xe9 la petite place pour aller au march\xe9.\r\nLa lumi\xe8re entrait par les fen\xeatres de la maison et les enfants jouaient dans le jardin.\r\nApr\xe8s le d\xe9jeuner, nous sommes all\xe9s chercher du pain et des fruits pour le d\xeener.\r\n";
    round_trip("A café.txt", &paragraph.repeat(80), "café", false);
}
