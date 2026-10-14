use kaia_storage::{DocumentId, InkPoint, InkStroke, Store, DB_FILE, MIGRATIONS};

#[test]
fn ink_migration_preserves_words_and_round_trips_strokes() {
    let dir = tempfile::tempdir().unwrap();
    let mut old = Store::open_with_migrations(dir.path(), &MIGRATIONS[..4]).unwrap();
    old.add_word("remember", None, None, 1).unwrap();
    drop(old);
    let mut store = Store::open(dir.path()).unwrap();
    let conn = rusqlite::Connection::open(dir.path().join(DB_FILE)).unwrap();
    assert_eq!(
        conn.pragma_query_value::<u32, _>(None, "user_version", |r| r.get(0))
            .unwrap(),
        5
    );
    assert_eq!(store.list_words().unwrap()[0].word, "remember");
    assert_eq!(std::fs::read_dir(dir.path().join("backups")).unwrap().count(), 1);
    let doc = DocumentId {
        partial_md5: "a".repeat(32),
        file_size: 100,
        title: Some("Book".into()),
    };
    let mut stroke = InkStroke {
        id: 0,
        page: 3,
        locator: Some("/body/p[1]".into()),
        points: vec![InkPoint {
            x: 1.0,
            y: 2.0,
            pressure: 55,
        }],
        width: 3.0,
        color: "black".into(),
    };
    stroke.id = store.add_ink(&doc, &stroke).unwrap();
    drop(store);
    let mut store = Store::open(dir.path()).unwrap();
    assert_eq!(store.ink(&doc.partial_md5).unwrap(), vec![stroke.clone()]);
    assert_eq!(store.ink_counts().unwrap(), vec![(doc.clone(), 3, 1)]);
    store.delete_ink(&"b".repeat(32), &[stroke.id]).unwrap();
    assert_eq!(store.ink(&doc.partial_md5).unwrap().len(), 1);
    store.delete_ink(&doc.partial_md5, &[stroke.id]).unwrap();
    assert!(store.ink(&doc.partial_md5).unwrap().is_empty());
    stroke.points[0].x = f32::NAN;
    assert!(store.add_ink(&doc, &stroke).is_err());
}
