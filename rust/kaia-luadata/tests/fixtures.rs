//! Round trips on the data files in `tests/fixtures`. The files come from
//! KOReader's emulator data and from sidecars with book text replaced.

mod common;

use std::fs;
use std::path::PathBuf;

use common::{same, scratch_dir};
use kaia_luadata::{parse, to_string, write_file, Value};

fn fixture_paths() -> Vec<PathBuf> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut paths: Vec<PathBuf> = fs::read_dir(&dir)
        .expect("fixtures directory")
        .map(|e| e.expect("directory entry").path())
        .filter(|p| p.extension().is_some_and(|e| e == "lua"))
        .collect();
    paths.sort();
    assert!(
        paths.len() >= 5,
        "expected the fixture files, found {}",
        paths.len()
    );
    paths
}

#[test]
fn every_fixture_round_trips_through_to_string() {
    for path in fixture_paths() {
        let src = fs::read_to_string(&path).unwrap();
        let value = parse(&src).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let text = to_string(&value);
        let back = parse(&text).unwrap_or_else(|e| panic!("{}: reparse: {e}\n{text}", path.display()));
        assert!(same(&value, &back), "{} changed in a round trip", path.display());
        assert_eq!(to_string(&back), text, "{} is not stable", path.display());
    }
}

#[test]
fn every_fixture_round_trips_through_write_file() {
    let dir = scratch_dir("fixtures");
    for path in fixture_paths() {
        let src = fs::read_to_string(&path).unwrap();
        let value = parse(&src).unwrap();
        let target = dir.join(path.file_name().unwrap());
        write_file(&target, &value).unwrap();
        let written = fs::read_to_string(&target).unwrap();
        let first_line = written.lines().next().unwrap();
        assert_eq!(first_line, format!("-- {}", target.display()));
        assert!(same(&value, &parse(&written).unwrap()));
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn sidecar_annotations_have_the_expected_fields() {
    let src = fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sidecar-annotations.lua"),
    )
    .unwrap();
    let value = parse(&src).unwrap();
    assert_eq!(value.get("doc_path").as_str(), Some("/books/sample.epub"));
    let annotations = value.get("annotations");
    assert_eq!(annotations.array_len(), 3);
    let first = annotations.get(1i64);
    assert_eq!(first.get("text").as_str(), Some("Placeholder text 1."));
    assert_eq!(first.get("pageno").as_i64(), Some(1));
    assert_eq!(
        first.get("pos0").as_str(),
        Some("/body/DocFragment[2]/body/div/p[1].0")
    );
}

#[test]
fn battery_stats_numbers_keep_their_values() {
    let src = fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/settings-battery-stats.lua"),
    )
    .unwrap();
    let value = parse(&src).unwrap();
    // KOReader writes this with %.14g, so the stored value is 14 digits long.
    let ts = value.get("charging_state").get("timestamp").as_f64().unwrap();
    assert_eq!(ts, 1.791870483627e15);
    assert!(matches!(value, Value::Table(_)));
}
