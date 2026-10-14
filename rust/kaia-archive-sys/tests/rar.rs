mod common;
use kaia_archive_sys::{Reader, MAX_ENTRIES, MAX_ENTRY_BYTES};
use std::io::Write;

fn file(bytes: &[u8]) -> tempfile::NamedTempFile {
    let mut file = tempfile::NamedTempFile::new().unwrap();
    file.write_all(bytes).unwrap();
    file
}

fn decode_uu(text: &str) -> Vec<u8> {
    let mut output = Vec::new();
    for line in text
        .lines()
        .skip_while(|line| !line.starts_with("begin "))
        .skip(1)
    {
        if line == "end" {
            break;
        }
        let line = line.as_bytes();
        let count = ((line[0] - 32) & 63) as usize;
        let start = output.len();
        for chunk in line[1..].as_chunks::<4>().0.iter() {
            let b: Vec<u8> = chunk.iter().map(|v| v.wrapping_sub(32) & 63).collect();
            output.extend([b[0] << 2 | b[1] >> 4, b[1] << 4 | b[2] >> 2, b[2] << 6 | b[3]]);
        }
        output.truncate(start + count);
    }
    output
}

#[test]
fn reads_stored_rar4_and_drains_skipped_entries() {
    let file = file(&common::rar(&[("a.txt", b"first", 5), ("b.txt", b"second", 6)]));
    let mut reader = Reader::open(file.path()).unwrap();
    assert_eq!(reader.next_entry().unwrap().unwrap().name, "a.txt");
    assert_eq!(reader.next_entry().unwrap().unwrap().name, "b.txt");
    assert_eq!(reader.read_entry().unwrap(), b"second");
    assert!(reader.next_entry().unwrap().is_none());
}

#[test]
fn upstream_rar5_stored_compressed_and_solid_vectors() {
    for (fixture, names, size) in [
        (
            include_str!("fixtures/test_read_format_rar5_stored.rar.uu"),
            vec!["helloworld.txt"],
            29,
        ),
        (
            include_str!("fixtures/test_read_format_rar5_compressed.rar.uu"),
            vec!["test.bin"],
            1200,
        ),
        (
            include_str!("fixtures/test_read_format_rar5_multiple_files_solid.rar.uu"),
            vec!["test1.bin", "test2.bin", "test3.bin", "test4.bin"],
            4096,
        ),
    ] {
        let file = file(&decode_uu(fixture));
        let mut reader = Reader::open(file.path()).unwrap();
        let solid = names.len() == 4;
        for (index, name) in names.into_iter().enumerate() {
            let entry = reader.next_entry().unwrap().unwrap();
            assert_eq!(entry.name, name);
            let data = reader.read_entry().unwrap();
            assert_eq!(data.len(), size);
            if size == 29 {
                assert_eq!(data, b"hello libarchive test suite!\n");
            } else {
                let magic = if solid { index as i32 + 1 } else { 0 };
                for (i, bytes) in data.as_chunks::<4>().0.iter().enumerate() {
                    let k = i as i32 + 1;
                    assert_eq!(i32::from_le_bytes(*bytes), (k * k - 3 * k + 1 + magic).max(0));
                }
            }
        }
        assert!(reader.next_entry().unwrap().is_none());
    }
}

#[test]
fn rejects_traversal_absolute_windows_and_long_paths() {
    for name in [
        "../page.png",
        "dir/../page.png",
        "/page.png",
        "\\page.png",
        "C:\\page.png",
        "dir\\..\\page.png",
        &"a".repeat(4097),
    ] {
        let file = file(&common::rar(&[(name, b"x", 1)]));
        let mut reader = Reader::open(file.path()).unwrap();
        assert!(reader.next_entry().is_err(), "{name}");
        assert!(reader.next_entry().is_err(), "failed reader must stay failed");
    }
}

#[test]
fn rejects_oversize_entries_before_reading_data() {
    let file = file(&common::rar(&[("page.png", b"", MAX_ENTRY_BYTES as u32 + 1)]));
    let mut reader = Reader::open(file.path()).unwrap();
    let error = reader.next_entry().unwrap_err();
    assert!(error.to_string().contains("64 MiB"), "{error}");
}

#[test]
fn rejects_too_many_entries() {
    let names: Vec<_> = (0..=MAX_ENTRIES).map(|i| format!("empty{i}")).collect();
    let entries: Vec<_> = names
        .iter()
        .map(|name| (name.as_str(), b"".as_slice(), 0))
        .collect();
    let file = file(&common::rar(&entries));
    let mut reader = Reader::open(file.path()).unwrap();
    for _ in 0..MAX_ENTRIES {
        reader.next_entry().unwrap().unwrap();
    }
    assert!(reader.next_entry().unwrap_err().to_string().contains("count"));
}

#[test]
fn rejects_truncation_and_false_sizes() {
    let bytes = common::rar(&[("page.png", b"abcdef", 6)]);
    let mut truncated = bytes.clone();
    truncated.truncate(bytes.len() - 10);
    for (case, bytes) in [
        truncated,
        common::rar(&[("page.png", b"abc", 6)]),
        common::rar(&[("page.png", b"abcdef", 3)]),
    ]
    .into_iter()
    .enumerate()
    {
        let file = file(&bytes);
        let mut reader = Reader::open(file.path()).unwrap();
        reader.next_entry().unwrap().unwrap();
        assert!(reader.read_entry().is_err(), "case {case}");
        assert!(reader.next_entry().is_err());
    }
}

#[test]
fn truncated_upstream_rar5_fails() {
    let mut bytes = decode_uu(include_str!("fixtures/test_read_format_rar5_compressed.rar.uu"));
    bytes.truncate(bytes.len() - 30);
    let file = file(&bytes);
    let mut reader = Reader::open(file.path()).unwrap();
    reader.next_entry().unwrap().unwrap();
    assert!(reader.read_entry().is_err());
}

#[test]
fn rejects_other_archive_formats_and_missing_files() {
    assert!(Reader::open(std::path::Path::new("/nonexistent/kaia.cbr")).is_err());
    let file = file(b"PK\x03\x04not a rar archive");
    assert!(Reader::open(file.path()).is_err());
}

#[test]
fn rejects_corrupt_header_crc() {
    let mut bytes = common::rar(&[("page.png", b"data", 4)]);
    bytes[20] ^= 1; // The first file header CRC follows the 7-byte signature and 13-byte main header.
    let file = file(&bytes);
    let mut reader = Reader::open(file.path()).unwrap();
    assert!(reader.next_entry().is_err());
}
