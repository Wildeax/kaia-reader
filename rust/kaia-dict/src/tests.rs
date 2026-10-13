//! End-to-end tests. Each test writes StarDict files with `testgen` and uses the public API.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::dictzip::{DataFile, DictZip};
use crate::testgen::{dictzip, sample_words, scratch, Gen, GenEntry};
use crate::{DefinitionKind, Dictionary, Entry, Error, Library, LookupOptions, Match};

fn open(gen: &Gen, dir: &Path) -> Dictionary {
    Dictionary::open(gen.write(dir)).unwrap()
}

fn headwords(entries: &[Entry]) -> Vec<String> {
    let mut words: Vec<String> = entries.iter().map(|e| e.headword.clone()).collect();
    words.sort();
    words
}

fn definition_for(word: &str) -> String {
    format!("Definition of {word}. Lorem ipsum dolor sit amet, consectetur adipiscing elit.")
}

#[test]
fn every_word_is_found_across_offset_widths_and_compression() {
    let words = sample_words(400);
    let entries: Vec<GenEntry> = words
        .iter()
        .map(|w| (w.clone(), vec![(b'm', definition_for(w).into_bytes())]))
        .collect();
    let variants = [
        ("plain", None, false, false),
        ("dz-small-chunks", Some(300), false, false),
        ("dz-gzip-idx-64", Some(300), true, true),
        ("dz-large-chunks-64", Some(60_000), true, false),
    ];
    for (name, chunk, idx64, gzip_idx) in variants {
        let gen = Gen {
            name: "round".into(),
            entries: entries.clone(),
            idx64,
            gzip_idx,
            dictzip_chunk: chunk,
            same_type: None,
            syn: Vec::new(),
        };
        let dict = open(&gen, &scratch(&format!("round-{name}")));
        assert_eq!(dict.len(), words.len(), "{name}");
        for word in &words {
            let found = dict.lookup(word);
            assert_eq!(found.len(), 1, "{name}: {word}");
            assert_eq!(found[0].headword, *word, "{name}");
            assert_eq!(found[0].definition.text, definition_for(word), "{name}");
            assert_eq!(found[0].matched, Match::Exact, "{name}");
            assert_eq!(found[0].dict_name, "round", "{name}");
        }
    }
}

#[test]
fn exact_match_comes_before_case_insensitive_matches() {
    let gen = Gen::plain(
        "case",
        &[
            ("Apple", "capital"),
            ("apple", "fruit"),
            ("APPLE", "shout"),
            ("banana", "yellow"),
        ],
    );
    let dict = open(&gen, &scratch("case"));

    let exact = dict.lookup("apple");
    assert_eq!(exact.len(), 1);
    assert_eq!(exact[0].definition.text, "fruit");
    assert_eq!(exact[0].matched, Match::Exact);

    assert_eq!(dict.lookup("Apple")[0].definition.text, "capital");
    assert_eq!(dict.lookup("APPLE")[0].definition.text, "shout");

    let folded = dict.lookup("aPPle");
    assert_eq!(headwords(&folded), ["apple"]);
    assert!(folded.iter().all(|e| e.matched == Match::CaseInsensitive));
}

#[test]
fn unicode_case_forms_match_accented_headwords() {
    let gen = Gen::plain("accent", &[("Épuisant", "adj. exhausting")]);
    let dict = open(&gen, &scratch("accent"));
    let found = dict.lookup("épuisant");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].headword, "Épuisant");
    assert_eq!(found[0].matched, Match::CaseInsensitive);
}

#[test]
fn synonyms_resolve_to_their_target_headword() {
    let mut gen = Gen::plain("syn", &[("color", "the visual property"), ("cat", "small pet")]);
    gen.syn = vec![("colour".into(), "color".into()), ("Kitty".into(), "cat".into())];
    let dict = open(&gen, &scratch("syn"));

    let found = dict.lookup("Colour");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].headword, "color");
    assert_eq!(found[0].definition.text, "the visual property");
    assert_eq!(found[0].matched, Match::Synonym);

    assert_eq!(dict.lookup("kitty")[0].headword, "cat");
    assert!(dict
        .lookup_with(
            "dog",
            &LookupOptions {
                fuzzy: false,
                ..LookupOptions::default()
            }
        )
        .is_empty());
}

#[test]
fn morphology_finds_the_headword_for_inflected_forms() {
    let gen = Gen::plain("morph", &[("dog", "a domestic animal"), ("box", "a container")]);
    let dict = open(&gen, &scratch("morph"));
    let dogs = dict.lookup("dogs");
    assert_eq!(dogs.len(), 1);
    assert_eq!(dogs[0].headword, "dog");
    assert_eq!(dogs[0].matched, Match::Morphology);
    assert_eq!(dict.lookup("boxes")[0].headword, "box");
}

#[test]
fn fuzzy_matches_prefixes_and_single_edits() {
    let gen = Gen::plain(
        "fuzzy",
        &[
            ("épuisant", "adj. exhausting"),
            ("hello", "greeting"),
            ("help", "assistance"),
        ],
    );
    let dict = open(&gen, &scratch("fuzzy"));

    let prefix = dict.lookup("épuisantes");
    assert_eq!(headwords(&prefix), ["épuisant"]);
    assert_eq!(prefix[0].matched, Match::Fuzzy);

    let edits = dict.lookup("helo");
    assert_eq!(headwords(&edits), ["hello", "help"]);
    assert!(edits.iter().all(|e| e.matched == Match::Fuzzy));

    let off = LookupOptions {
        fuzzy: false,
        ..LookupOptions::default()
    };
    assert!(dict.lookup_with("helo", &off).is_empty());

    let capped = LookupOptions {
        max_fuzzy: 1,
        ..LookupOptions::default()
    };
    assert_eq!(dict.lookup_with("helo", &capped).len(), 1);

    assert!(dict.lookup("zzzzzz").is_empty());
    assert!(dict.lookup("   ").is_empty());
}

#[test]
fn exact_hits_hide_fuzzy_results() {
    let gen = Gen::plain("stage", &[("hello", "greeting"), ("helo", "typo entry")]);
    let dict = open(&gen, &scratch("stage"));
    let found = dict.lookup("helo");
    assert_eq!(headwords(&found), ["helo"]);
    assert_eq!(found[0].matched, Match::Exact);
}

#[test]
fn library_loads_nested_folders_and_reports_broken_dictionaries() {
    let root = scratch("library");
    Gen::plain("Alpha", &[("word", "alpha definition")]).write(&root.join("a"));
    Gen::plain("Beta", &[("word", "beta definition")]).write(&root.join("b"));
    let broken = Gen::plain("Broken", &[("word", "never read")]).write(&root.join("broken"));
    fs::remove_file(broken.with_extension("idx")).unwrap();
    fs::write(root.join("notes.txt"), "not a dictionary").unwrap();

    let library = Library::open_dir(&root).unwrap();
    assert_eq!(library.dictionaries().len(), 2);
    assert_eq!(library.errors().len(), 1);
    assert!(matches!(library.errors()[0].1, Error::Missing { .. }));

    let all = library.lookup("word");
    let names: Vec<&str> = all.iter().map(|e| e.dict_name.as_str()).collect();
    assert_eq!(names, ["Alpha", "Beta"]);

    let only_beta = LookupOptions {
        dictionaries: Some(vec!["Beta".into()]),
        exclusive: true,
        ..LookupOptions::default()
    };
    let beta = library.lookup_with("word", &only_beta);
    assert_eq!(beta.len(), 1);
    assert_eq!(beta[0].definition.text, "beta definition");

    assert!(matches!(
        Library::open_dir(root.join("missing")),
        Err(Error::Io { .. })
    ));
}

#[test]
fn preferred_order_disabled_and_global_fuzzy_fallback() {
    let root = scratch("order-and-fuzzy");
    Gen::plain("Zulu", &[("apple", "fruit")]).write(&root.join("a"));
    Gen::plain("Alpha", &[("ample", "enough")]).write(&root.join("b"));
    Gen::plain("Beta", &[("apple", "another fruit")]).write(&root.join("c"));
    let library = Library::open_dir(root).unwrap();
    let options = LookupOptions {
        dictionaries: Some(vec!["Zulu".into()]),
        ..LookupOptions::default()
    };
    let names: Vec<_> = library
        .lookup_with("apple", &options)
        .iter()
        .map(|e| e.dict_name.clone())
        .collect();
    assert_eq!(names, ["Zulu", "Beta"]);
    let fuzzy = library.lookup_with("aple", &options);
    assert!(fuzzy.iter().any(|e| e.dict_name == "Alpha"));
    let exact = library.lookup_with("apple", &options);
    assert!(exact.iter().all(|e| e.matched != Match::Fuzzy));
    let options = LookupOptions {
        disabled: ["Zulu".into()].into(),
        ..options
    };
    assert_eq!(library.lookup_with("apple", &options)[0].dict_name, "Beta");
}

#[test]
fn exact_mode_uses_global_exact_before_lowercase() {
    let root = scratch("exact-global");
    Gen::plain("One", &[("APPLE", "upper")]).write(&root.join("a"));
    Gen::plain("Two", &[("apple", "lower")]).write(&root.join("b"));
    let library = Library::open_dir(root).unwrap();
    let options = LookupOptions {
        fuzzy: false,
        ..LookupOptions::default()
    };
    assert_eq!(library.lookup_with("APPLE", &options)[0].dict_name, "One");
    assert_eq!(library.lookup_with("APPLE", &options).len(), 1);
    assert!(library.lookup_with("Apple", &options).is_empty());
}

#[test]
fn compressed_files_take_precedence_and_index_size_is_checked() {
    let dir = scratch("preferred-files");
    let mut gen = Gen::plain("Preferred", &[("word", "compressed")]);
    gen.gzip_idx = true;
    gen.dictzip_chunk = Some(32);
    let ifo = gen.write(&dir);
    fs::write(ifo.with_extension("idx"), b"broken index").unwrap();
    fs::write(ifo.with_extension("dict"), b"wrong definition").unwrap();
    assert_eq!(
        Dictionary::open(&ifo).unwrap().lookup("word")[0].definition.text,
        "compressed"
    );
    let original = fs::read_to_string(&ifo).unwrap();
    let bad = original.replace("idxfilesize=13", "idxfilesize=99");
    assert_ne!(original, bad);
    fs::write(&ifo, bad).unwrap();
    assert!(Dictionary::open(&ifo).is_err());
}

#[cfg(unix)]
#[test]
fn scan_follows_links_and_skips_resource_folders() {
    use std::os::unix::fs::symlink;
    let root = scratch("links-and-resources");
    let outside = scratch("linked-dictionary");
    Gen::plain("Linked", &[("word", "found")]).write(&outside);
    Gen::plain("Resource", &[("word", "hidden")]).write(&root.join("res"));
    fs::create_dir_all(&root).unwrap();
    symlink(&outside, root.join("linked")).unwrap();
    let library = Library::open_dir(root).unwrap();
    assert_eq!(library.dictionaries().len(), 1);
    assert_eq!(library.dictionaries()[0].name(), "Linked");
}

#[test]
fn each_dictionary_reports_its_own_first_matching_stage() {
    // The exact match in one dictionary must not hide the inflected match in another,
    // which is how sdcv behaves.
    let root = scratch("library-stages");
    Gen::plain("Walk", &[("walk", "to move on foot")]).write(&root.join("1"));
    Gen::plain("Walking", &[("walking", "present participle")]).write(&root.join("2"));
    let library = Library::open_dir(&root).unwrap();
    let found = library.lookup("walking");
    let mut pairs: Vec<(&str, Match)> = found.iter().map(|e| (e.headword.as_str(), e.matched)).collect();
    pairs.sort_by(|a, b| a.0.cmp(b.0));
    assert_eq!(pairs, [("walk", Match::Morphology), ("walking", Match::Exact)]);
}

#[test]
fn type_sequence_joins_text_fields_and_skips_binary_fields() {
    let mut gen = Gen::plain("seq", &[]);
    gen.same_type = Some("tmW".into());
    gen.entries = vec![(
        "apple".into(),
        vec![
            (b't', "ˈæpəl".as_bytes().to_vec()),
            (b'm', b"fruit".to_vec()),
            (b'W', vec![1, 2, 3]),
        ],
    )];
    let dict = open(&gen, &scratch("seq"));
    let found = dict.lookup("apple");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].definition.kind, DefinitionKind::Phonetic);
    assert_eq!(found[0].definition.text, "[ˈæpəl]\nfruit");
}

#[test]
fn html_and_other_types_keep_their_kind() {
    let gen = Gen {
        name: "kinds".into(),
        entries: vec![
            ("bold".into(), vec![(b'h', b"<b>bold</b><p>para</p>".to_vec())]),
            ("plain".into(), vec![(b'm', b"text".to_vec())]),
            ("xml".into(), vec![(b'x', b"<ar><k>xml</k></ar>".to_vec())]),
            ("odd".into(), vec![(b'k', b"other".to_vec())]),
            ("resource".into(), vec![(b'r', b"img.png".to_vec())]),
            ("picture".into(), vec![(b'P', vec![0xff, 0xd8, 0xff])]),
        ],
        idx64: false,
        gzip_idx: false,
        dictzip_chunk: None,
        same_type: None,
        syn: Vec::new(),
    };
    let dict = open(&gen, &scratch("kinds"));

    let bold = dict.lookup("bold");
    assert_eq!(bold[0].definition.kind, DefinitionKind::Html);
    assert_eq!(bold[0].definition.plain_text(), "bold\n\npara");

    assert_eq!(dict.lookup("plain")[0].definition.kind, DefinitionKind::Plain);
    assert_eq!(dict.lookup("xml")[0].definition.kind, DefinitionKind::Xdxf);
    assert_eq!(dict.lookup("xml")[0].definition.plain_text(), "[]");
    assert_eq!(dict.lookup("odd")[0].definition.kind, DefinitionKind::Other('k'));
    assert!(dict.lookup("resource").is_empty());
    assert!(dict.lookup("picture").is_empty());
}

#[test]
fn dictzip_reads_ranges_across_chunks_and_inflates_only_what_it_needs() {
    let data: Vec<u8> = (0..10_000u32).map(|i| (i * 31 % 251) as u8).collect();
    let dir = scratch("dz-ranges");
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("x.dict.dz");
    fs::write(&path, dictzip(&data, 1000)).unwrap();

    let file = DataFile::DictZip(DictZip::open(&path).unwrap());
    assert_eq!(file.len(), 10_000);
    let DataFile::DictZip(zip) = &file else {
        unreachable!("opened as dictzip")
    };
    assert_eq!(zip.chunks_inflated(), 0);

    assert_eq!(file.read(5100, 50).unwrap(), data[5100..5150]);
    assert_eq!(zip.chunks_inflated(), 1);

    assert_eq!(file.read(990, 30).unwrap(), data[990..1020]);
    assert_eq!(zip.chunks_inflated(), 3);

    assert_eq!(file.read(9990, 10).unwrap(), data[9990..]);
    assert_eq!(file.read(0, 0).unwrap(), Vec::<u8>::new());
    assert!(file.read(9995, 10).is_err());
}

#[test]
fn dictzip_rejects_gzip_without_ra_and_unknown_versions() {
    let dir = scratch("dz-bad");
    fs::create_dir_all(&dir).unwrap();
    let plain_gzip = dir.join("plain.dict.dz");
    let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    std::io::Write::write_all(&mut enc, b"hello").unwrap();
    fs::write(&plain_gzip, enc.finish().unwrap()).unwrap();
    assert!(DictZip::open(&plain_gzip).is_err());

    let mut bytes = dictzip(b"some text to store", 8);
    let ra = bytes.windows(2).position(|w| w == b"RA").unwrap();
    bytes[ra + 4] = 2; // RA version field
    let bad_version = dir.join("version.dict.dz");
    fs::write(&bad_version, bytes).unwrap();
    assert!(DictZip::open(&bad_version).is_err());
}

#[test]
fn definitions_larger_than_one_chunk_read_whole() {
    let big = "x".repeat(5000);
    let gen = Gen {
        name: "big".into(),
        entries: vec![("huge".into(), vec![(b'm', big.clone().into_bytes())])],
        idx64: false,
        gzip_idx: false,
        dictzip_chunk: Some(256),
        same_type: None,
        syn: Vec::new(),
    };
    let dict = open(&gen, &scratch("big"));
    assert_eq!(dict.lookup("huge")[0].definition.text, big);
}

#[test]
fn corrupt_or_missing_files_return_errors_without_panicking() {
    let entries = [("alpha", "one"), ("beta", "two"), ("gamma", "three")];
    let base_gen = || Gen::plain("bad", &entries);

    // Missing data file.
    let ifo = base_gen().write(&scratch("bad-missing-dict"));
    fs::remove_file(ifo.with_extension("dict")).unwrap();
    assert!(matches!(Dictionary::open(&ifo), Err(Error::Missing { .. })));

    // Data file shorter than the index claims.
    let ifo = base_gen().write(&scratch("bad-short-dict"));
    let dict_bytes = fs::read(ifo.with_extension("dict")).unwrap();
    fs::write(ifo.with_extension("dict"), &dict_bytes[..4]).unwrap();
    assert!(Dictionary::open(&ifo).is_err());

    // Truncated index.
    let ifo = base_gen().write(&scratch("bad-short-idx"));
    let idx = fs::read(ifo.with_extension("idx")).unwrap();
    fs::write(ifo.with_extension("idx"), &idx[..idx.len() - 3]).unwrap();
    assert!(Dictionary::open(&ifo).is_err());

    // Not an ifo file.
    let ifo = base_gen().write(&scratch("bad-ifo"));
    fs::write(&ifo, "bookname=Missing header\n").unwrap();
    assert!(Dictionary::open(&ifo).is_err());

    // Synonym that points past the index.
    let mut gen = base_gen();
    gen.syn = vec![("alef".into(), "alpha".into())];
    let ifo = gen.write(&scratch("bad-syn"));
    let mut syn = fs::read(ifo.with_extension("syn")).unwrap();
    let n = syn.len();
    syn[n - 4..].copy_from_slice(&999u32.to_be_bytes());
    fs::write(ifo.with_extension("syn"), syn).unwrap();
    assert!(Dictionary::open(&ifo).is_err());

    // Corrupt dictzip header.
    let mut gen = base_gen();
    gen.dictzip_chunk = Some(8);
    let ifo = gen.write(&scratch("bad-dz"));
    let mut dz = fs::read(ifo.with_extension("dict.dz")).unwrap();
    dz[0] = 0;
    fs::write(ifo.with_extension("dict.dz"), dz).unwrap();
    assert!(Dictionary::open(&ifo).is_err());
}

/// Overwrites each file of a small dictionary with every truncation and every single-byte
/// flip. Opening and looking up must return a value or an error, never panic.
#[test]
fn damaged_bytes_never_panic_on_open_or_lookup() {
    let entries = [
        ("alpha", "one"),
        ("beta", "two"),
        ("gamma", "three"),
        ("delta", "four"),
    ];
    let mut gen = Gen::plain("fuzz", &entries);
    gen.dictzip_chunk = Some(10);
    gen.syn = vec![("alef".into(), "alpha".into())];
    let source = scratch("fuzz-source");
    let ifo = gen.write(&source);

    let work = scratch("fuzz-work");
    fs::create_dir_all(&work).unwrap();
    let work_ifo = work.join("fuzz.ifo");
    fs::copy(&ifo, &work_ifo).unwrap();
    for ext in ["idx", "dict.dz", "syn"] {
        let original = fs::read(ifo.with_extension(ext)).unwrap();
        let dest: PathBuf = work.join(format!("fuzz.{ext}"));
        let probe = |dest_bytes: &[u8]| {
            fs::write(&dest, dest_bytes).unwrap();
            if let Ok(dict) = Dictionary::open(&work_ifo) {
                let _ = dict.lookup("alpha");
                let _ = dict.lookup("alef");
                let _ = dict.lookup("gamm");
            }
        };
        for len in 0..original.len() {
            probe(&original[..len]);
        }
        for pos in 0..original.len() {
            let mut flipped = original.clone();
            flipped[pos] ^= 0xff;
            probe(&flipped);
        }
        fs::write(&dest, &original).unwrap();
    }
}

#[test]
fn unsorted_index_files_are_sorted_on_load() {
    let entries = [("zebra", "z"), ("Apple", "a"), ("mango", "m")];
    let dict = open(&Gen::plain("unsorted", &entries), &scratch("unsorted"));
    assert_eq!(dict.lookup("mango")[0].definition.text, "m");
    assert_eq!(dict.lookup("APPLE")[0].headword, "Apple");
}

/// Index of 500k words. Run in release mode: `cargo test -p kaia-dict --release -- --ignored`.
#[test]
#[ignore]
fn index_of_500k_words_opens_in_under_a_second_in_release() {
    let count = 500_000;
    let entries: Vec<GenEntry> = (0..count)
        .map(|i| {
            (
                format!("word{i:06}"),
                vec![(b'm', format!("definition {i}").into_bytes())],
            )
        })
        .collect();
    let gen = Gen {
        name: "big".into(),
        entries,
        idx64: false,
        gzip_idx: false,
        dictzip_chunk: None,
        same_type: None,
        syn: Vec::new(),
    };
    let ifo = gen.write(&scratch("perf"));

    let start = Instant::now();
    let dict = Dictionary::open(&ifo).unwrap();
    let open_time = start.elapsed();
    assert_eq!(dict.len(), count);

    let probes = 10_000;
    let start = Instant::now();
    for i in 0..probes {
        let n = (i * 7919) % count;
        let found = dict.lookup(&format!("word{n:06}"));
        assert_eq!(found[0].definition.text, format!("definition {n}"));
    }
    let lookup_time = start.elapsed() / probes as u32;

    eprintln!("open 500k words: {open_time:?}; mean exact lookup: {lookup_time:?}");
    if !cfg!(debug_assertions) {
        assert!(open_time.as_secs_f64() < 1.0, "open took {open_time:?}");
    }
}

/// Reads every record of the real dictionaries named by `KAIA_TEST_DICTS`, which checks
/// that the whole dictzip decodes. Run with `--ignored`.
#[test]
#[ignore]
fn every_record_of_real_dictionaries_reads() {
    let Some(dir) = std::env::var_os("KAIA_TEST_DICTS") else {
        eprintln!("KAIA_TEST_DICTS is not set; nothing to read");
        return;
    };
    let library = Library::open_dir(&dir).unwrap();
    assert!(library.errors().is_empty(), "{:?}", library.errors());
    for dict in library.dictionaries() {
        let start = Instant::now();
        let unreadable = (0..dict.index.len())
            .filter(|&rec| dict.entry(rec, Match::Exact).is_none())
            .count();
        eprintln!(
            "{}: {} records, {} without readable text, {:?}",
            dict.name(),
            dict.len(),
            unreadable,
            start.elapsed()
        );
        assert_eq!(unreadable, 0, "{}", dict.name());
    }
}
