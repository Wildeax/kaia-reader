//! The engine's document table is process-wide, so this test has its own binary.

use std::path::Path;

use kaia_cre_sys::{init_with_options, registered_fonts, Doc, RuntimeOptions};

#[test]
fn document_capacity_preserves_fonts_and_recovers_after_drop() {
    init_with_options(&RuntimeOptions::default()).unwrap();
    let fonts = registered_fonts().unwrap();
    assert!(!fonts.is_empty());
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test/leaves.epub");
    let mut docs = Vec::new();
    let mut refused = false;
    let mut loaded_books = Vec::new();
    for _ in 0..32 {
        match Doc::new(200, 300) {
            Ok(mut doc) => {
                let loaded = doc.load(&path).unwrap();
                loaded_books.push(loaded);
                if loaded {
                    assert!(doc.page_count().unwrap() > 0);
                }
                eprintln!("loaded {} live documents: {loaded}", docs.len() + 1);
                docs.push(doc);
            }
            Err(error) => {
                assert!(error.to_string().contains("document capacity"), "{error}");
                refused = true;
                break;
            }
        }
    }
    assert!(refused, "the shim must reserve slots for temporary documents");
    assert!(loaded_books.iter().all(|loaded| *loaded));
    assert_eq!(registered_fonts().unwrap(), fonts);
    drop(docs.pop());
    let mut replacement = Doc::new(200, 300).unwrap();
    assert!(replacement.load(&path).unwrap());
    assert!(replacement.page_count().unwrap() > 0);
    drop(replacement);
    drop(docs);
    assert_eq!(registered_fonts().unwrap(), fonts);
    let mut doc = Doc::new(200, 300).unwrap();
    assert!(doc.load(&path).unwrap());
}
