//! Compares lookups on a real StarDict folder with sdcv.
//!
//! Runs only when `KAIA_TEST_DICTS` names a folder of StarDict dictionaries, such as the
//! FreeDict English dictionaries. Without the variable the test returns at once. sdcv is taken
//! from `KAIA_SDCV`, else from the emulator build under the repository root, else from `PATH`.
//!
//! Run with `KAIA_TEST_DICTS=~/kaia-test-dicts cargo test -p kaia-dict --test real_dicts -- --nocapture`
//! to see the definitions.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use kaia_dict::Library;

const WORDS: [&str; 4] = ["house", "apple", "walking", "Running"];

fn find_sdcv() -> PathBuf {
    if let Some(path) = std::env::var_os("KAIA_SDCV") {
        return PathBuf::from(path);
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    if let Ok(entries) = std::fs::read_dir(&root) {
        let mut builds: Vec<PathBuf> = entries
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("koreader-emulator-"))
            })
            .collect();
        builds.sort();
        for build in builds {
            let candidate = build.join("koreader/sdcv");
            if candidate.is_file() {
                return candidate;
            }
        }
    }
    PathBuf::from("sdcv")
}

/// Headwords sdcv reports for `word` with its default (fuzzy) search.
fn sdcv_headwords(sdcv: &Path, dir: &Path, word: &str) -> BTreeSet<String> {
    let output = Command::new(sdcv)
        .args(["-n", "--json-output", "--data-dir"])
        .arg(dir)
        .arg("--")
        .arg(word)
        .output()
        .unwrap_or_else(|e| panic!("cannot run sdcv at {}: {e}", sdcv.display()));
    let text = String::from_utf8_lossy(&output.stdout);
    let text = text.trim();
    if text.is_empty() {
        return BTreeSet::new();
    }
    let items: Vec<serde_json::Value> =
        serde_json::from_str(text).unwrap_or_else(|e| panic!("sdcv printed bad JSON for {word}: {e}"));
    items
        .iter()
        .map(|item| item["word"].as_str().expect("word field").to_owned())
        .collect()
}

/// True when `text` contains something that looks like a markup tag.
fn has_tag(text: &str) -> bool {
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '<' && chars.peek().is_some_and(|n| n.is_ascii_alphabetic() || *n == '/') {
            return true;
        }
    }
    false
}

#[test]
fn headwords_match_sdcv_on_real_dictionaries() {
    let Some(dir) = std::env::var_os("KAIA_TEST_DICTS") else {
        eprintln!("KAIA_TEST_DICTS is not set; skipping the sdcv comparison");
        return;
    };
    let dir = PathBuf::from(dir);
    let sdcv = find_sdcv();

    let library = Library::open_dir(&dir).expect("open dictionary folder");
    if let Some((path, error)) = library.errors().first() {
        panic!("dictionary failed to open: {}: {error}", path.display());
    }
    assert!(
        !library.dictionaries().is_empty(),
        "no dictionaries under {}",
        dir.display()
    );

    for word in WORDS {
        let ours = library.lookup(word);
        let our_headwords: BTreeSet<String> = ours.iter().map(|e| e.headword.clone()).collect();
        let sdcv_set = sdcv_headwords(&sdcv, &dir, word);
        println!("== {word}: {} entries, headwords {our_headwords:?}", ours.len());
        for entry in &ours {
            let plain = entry.definition.plain_text();
            println!(
                "-- [{}] {} ({:?}, {:?})",
                entry.dict_name, entry.headword, entry.matched, entry.definition.kind
            );
            println!("{plain}");
            assert!(!has_tag(&plain), "markup left in {word}: {plain:.200}");
        }
        assert_eq!(our_headwords, sdcv_set, "headwords for {word} differ from sdcv");
    }
}
