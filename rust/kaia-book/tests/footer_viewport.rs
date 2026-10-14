//! The reader reserves its header and footer before it opens or resizes a book.

mod common;

use kaia_book::{BookSession, Engine};
use kaia_gfx::Size;

#[test]
fn both_reflow_engines_use_the_reduced_height_and_restore_it() {
    common::init_engine();
    let full = Size::new(1264, 1680);
    let reduced = Size::new(full.w, full.h - 2 * 56);
    let footer_only = Size::new(full.w, full.h - 56);
    for engine in [Engine::Cre, Engine::Mupdf] {
        let data = tempfile::tempdir().unwrap();
        let mut opts = common::options(&data);
        opts.engine = engine;
        opts.viewport = reduced;
        let mut session = BookSession::open(common::repo_file("juliet.epub"), opts).unwrap();
        assert_eq!(session.render_current(reduced).unwrap().size(), reduced);
        session.set_viewport(footer_only).unwrap();
        common::wait_layout(&mut session);
        assert_eq!(session.render_current(footer_only).unwrap().size(), footer_only);
        session.set_viewport(full).unwrap();
        common::wait_layout(&mut session);
        assert_eq!(session.render_current(full).unwrap().size(), full);
        session.set_viewport(reduced).unwrap();
        common::wait_layout(&mut session);
        assert_eq!(session.render_current(reduced).unwrap().size(), reduced);
    }
}
