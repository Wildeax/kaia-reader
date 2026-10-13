//! Checks that `include/kaia.h`, the LuaJIT declarations in
//! `frontend/kaia/ffi/kaia_h.lua`, and the exports in `src/lib.rs` describe the same ABI.

use std::collections::BTreeSet;

use regex::Regex;

const HEADER: &str = include_str!("../include/kaia.h");
const LUA_CDEF: &str = include_str!("../../../frontend/kaia/ffi/kaia_h.lua");
const LIB_RS: &str = include_str!("../src/lib.rs");

/// Collapses every run of whitespace to one space.
fn collapse(text: &str) -> String {
    Regex::new(r"\s+")
        .unwrap()
        .replace_all(text, " ")
        .trim()
        .to_string()
}

/// Every C prototype that names a `kaia_*` function, whitespace collapsed, each ending in `;`.
///
/// Comments and preprocessor lines are dropped. Braces become statement breaks,
/// so struct bodies do not join a prototype.
fn prototypes(text: &str) -> Vec<String> {
    let no_comments = Regex::new(r"(?s)/\*.*?\*/").unwrap().replace_all(text, " ");
    let code: String = no_comments
        .lines()
        .filter(|line| !line.trim_start().starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n");
    let named = Regex::new(r"\bkaia_\w+\s*\(").unwrap();
    code.replace(['{', '}'], ";")
        .split(';')
        .map(collapse)
        .filter(|statement| named.is_match(statement))
        .map(|statement| format!("{statement};"))
        .collect()
}

fn name_of(prototype: &str) -> String {
    Regex::new(r"\b(kaia_\w+)\s*\(")
        .unwrap()
        .captures(prototype)
        .expect("prototype has a name")[1]
        .to_string()
}

/// The text inside `ffi.cdef[[ ... ]]` in the Lua file.
fn lua_cdef_block() -> &'static str {
    let start = LUA_CDEF
        .find("ffi.cdef[[")
        .expect("kaia_h.lua has an ffi.cdef block")
        + "ffi.cdef[[".len();
    let end = start + LUA_CDEF[start..].find("]]").expect("cdef block is closed");
    &LUA_CDEF[start..end]
}

/// Names of the `pub extern "C" fn kaia_*` items in `src/lib.rs`.
fn exported_names() -> BTreeSet<String> {
    Regex::new(r#"pub (?:unsafe )?extern "C" fn (kaia_\w+)"#)
        .unwrap()
        .captures_iter(LIB_RS)
        .map(|c| c[1].to_string())
        .collect()
}

#[test]
fn every_header_function_is_exported_and_only_the_debug_panic_is_extra() {
    let declared: BTreeSet<String> = prototypes(HEADER).iter().map(|p| name_of(p)).collect();
    // ABI.md lists 19 functions. A change here means the ABI changed.
    assert_eq!(declared.len(), 19, "prototypes found in kaia.h");

    let exported = exported_names();
    for name in &declared {
        assert!(
            exported.contains(name),
            "{name} is in kaia.h but not exported from src/lib.rs"
        );
    }
    // kaia_debug_panic exists only with the test-panic feature and is not in the header.
    let extra: BTreeSet<String> = exported.difference(&declared).cloned().collect();
    let allowed: BTreeSet<String> = ["kaia_debug_panic".to_string()].into_iter().collect();
    assert_eq!(extra, allowed, "exports missing from kaia.h");
}

#[test]
fn lua_declares_every_header_function_with_the_same_signature() {
    let lua = collapse(lua_cdef_block());
    let header = prototypes(HEADER);
    assert!(!header.is_empty());
    for proto in &header {
        assert!(lua.contains(proto), "kaia_h.lua does not declare: {proto}");
    }
    let header_names: BTreeSet<String> = header.iter().map(|p| name_of(p)).collect();
    for proto in prototypes(lua_cdef_block()) {
        assert!(
            header_names.contains(&name_of(&proto)),
            "kaia_h.lua declares a function not in kaia.h: {proto}"
        );
    }
}

#[test]
fn header_error_codes_match_the_crate_constants() {
    let defines = Regex::new(r"(?m)^#define\s+(KAIA_\w+)\s+(-?\d+)\s*$").unwrap();
    let mut from_header: Vec<(String, i32)> = defines
        .captures_iter(HEADER)
        .map(|c| (c[1].to_string(), c[2].parse().expect("integer code")))
        .collect();
    from_header.sort();

    let mut expected: Vec<(String, i32)> = vec![
        ("KAIA_OK", kaia::KAIA_OK),
        ("KAIA_E_INVALID_ARG", kaia::KAIA_E_INVALID_ARG),
        ("KAIA_E_BAD_HANDLE", kaia::KAIA_E_BAD_HANDLE),
        ("KAIA_E_NOT_FOUND", kaia::KAIA_E_NOT_FOUND),
        ("KAIA_E_IO", kaia::KAIA_E_IO),
        ("KAIA_E_DB", kaia::KAIA_E_DB),
        ("KAIA_E_SCHEMA_TOO_NEW", kaia::KAIA_E_SCHEMA_TOO_NEW),
        ("KAIA_E_VALIDATION", kaia::KAIA_E_VALIDATION),
        ("KAIA_E_ASSET_REJECTED", kaia::KAIA_E_ASSET_REJECTED),
        ("KAIA_E_PANIC", kaia::KAIA_E_PANIC),
    ]
    .into_iter()
    .map(|(name, value)| (name.to_string(), value))
    .collect();
    expected.sort();
    assert_eq!(from_header, expected);
}

#[test]
fn kaia_abi_version_is_one() {
    assert_eq!(kaia::kaia_abi_version(), 1);
}
