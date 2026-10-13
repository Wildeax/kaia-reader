//! One process tests failed initialization and then retries with real resources.

use std::path::Path;

use kaia_cre_sys::{init, init_with_options, Doc, RuntimeOptions, DATA_DIR, FONTS_DIR};

#[test]
fn zero_usable_fonts_returns_an_error_and_allows_retry() {
    let hyph = Path::new(DATA_DIR).join("hyph");
    let error = init(&[], &hyph).unwrap_err();
    assert!(error.to_string().contains("no font files supplied"));
    assert!(Doc::new(600, 800).is_err());

    let missing = Path::new(FONTS_DIR).join("does-not-exist.ttf");
    let error = init(&[missing], &hyph).unwrap_err();
    assert!(error
        .to_string()
        .contains("none of the supplied files were accepted"));
    assert!(Doc::new(600, 800).is_err());

    let options = RuntimeOptions {
        data_dir: DATA_DIR.into(),
        font_dirs: vec![FONTS_DIR.into()],
    };
    assert!(init_with_options(&options).unwrap() > 0);
    assert!(Doc::new(600, 800).is_ok());
    // A bad resource configuration must also fail after a successful init.
    assert!(init(&[], &hyph).is_err());
}
