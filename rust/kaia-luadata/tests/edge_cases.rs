//! Hand-written inputs: syntax the parser must accept, and inputs it must
//! refuse with a position.

mod common;

use std::fs;

use common::{same, scratch_dir};
use kaia_luadata::{parse, to_string, write_file, Key, Table, Value, WriteError, MAX_DEPTH, MAX_INPUT_BYTES};

fn ok(src: &str) -> Value {
    parse(src).unwrap_or_else(|e| panic!("{src:?} should parse: {e}"))
}

fn err(src: &str) -> kaia_luadata::ParseError {
    match parse(src) {
        Ok(v) => panic!("{src:?} should fail, got {v:?}"),
        Err(e) => e,
    }
}

fn table(entries: Vec<(Key, Value)>) -> Value {
    Value::Table(Table::from_iter(entries))
}

#[test]
fn empty_and_scalar_chunks() {
    assert_eq!(ok("return {}"), Value::Table(Table::new()));
    assert_eq!(ok("return nil"), Value::Nil);
    assert_eq!(ok("return true"), Value::Bool(true));
    assert_eq!(ok("return false;"), Value::Bool(false));
    assert_eq!(ok("return 'x'"), Value::from("x"));
}

#[test]
fn comments_are_allowed_where_whitespace_is() {
    let src = "-- header\n--[[--\nblock\n]]\nreturn { --[==[ inline ]==] [1]=1 -- trailing\n} -- end";
    assert_eq!(ok(src), Value::array([Value::from(1i64)]));
}

#[test]
fn separators_and_trailing_separator() {
    assert_eq!(ok("return {1, 2; 3,}").array_len(), 3);
    assert_eq!(ok("return {1;2;3;}").array_len(), 3);
    assert!(parse("return {1;;}").is_err());
    assert!(parse("return {1 2}").is_err());
}

#[test]
fn positional_nil_keeps_its_index() {
    let v = ok("return {1, nil, 3}");
    assert_eq!(v.get(1i64), &Value::from(1i64));
    assert!(v.get(2i64).is_nil());
    assert_eq!(v.get(3i64), &Value::from(3i64));
}

#[test]
fn positional_entries_win_over_explicit_keys() {
    assert_eq!(ok("return {[1] = 'a', 'b'}").get(1i64), &Value::from("b"));
    assert_eq!(ok("return {'b', [1] = 'a'}").get(1i64), &Value::from("b"));
}

#[test]
fn nil_removes_and_later_keys_win() {
    assert_eq!(ok("return {[1] = 1, [1] = nil}"), Value::Table(Table::new()));
    assert_eq!(ok("return {a = 1, a = 2}").get("a"), &Value::from(2i64));
}

#[test]
fn key_values_are_read_back() {
    let t = match ok(
        "return {[1.5] = 'x', [-2] = 'y', [true] = 'z', ['a b'] = 'w', c = 'v', [0x10] = 'h', [1e2] = 'k'}",
    ) {
        Value::Table(t) => t,
        other => panic!("not a table: {other:?}"),
    };
    assert_eq!(t.get(&Key::num(1.5).unwrap()), Some(&Value::from("x")));
    assert_eq!(t.get(&Key::Int(-2)), Some(&Value::from("y")));
    assert_eq!(t.get(&Key::Bool(true)), Some(&Value::from("z")));
    assert_eq!(t.get(&Key::from("a b")), Some(&Value::from("w")));
    assert_eq!(t.get(&Key::from("c")), Some(&Value::from("v")));
    assert_eq!(t.get(&Key::Int(16)), Some(&Value::from("h")));
    assert_eq!(t.get(&Key::Int(100)), Some(&Value::from("k")));
}

#[test]
fn quoted_string_escapes() {
    assert_eq!(ok(r#"return "a\tb\\c\"d\'e\n""#), Value::from("a\tb\\c\"d'e\n"));
    assert_eq!(ok(r"return '\a\b\f\v\r'"), Value::from("\u{7}\u{8}\u{c}\u{b}\r"));
    assert_eq!(ok(r"return '\65\066\x43'"), Value::from("ABC"));
    assert_eq!(ok("return '\\0'"), Value::from("\0"));
    assert_eq!(ok("return 'a\\z  \n\t  b'"), Value::from("ab"));
    assert_eq!(ok("return 'a\\\nb'"), Value::from("a\nb"));
    assert_eq!(ok("return 'a\\\r\nb'"), Value::from("a\nb"));
    // Lua 5.1 and LuaJIT keep the character after an unknown escape.
    assert_eq!(ok(r"return '\q'"), Value::from("q"));
}

#[test]
fn bad_escapes_are_errors() {
    assert!(parse(r"return '\256'").is_err());
    assert!(parse(r"return '\xG1'").is_err());
    assert!(parse(r"return '\x4'").is_err());
    // Invalid UTF-8 bytes are replaced in the shared UTF-8 value type.
    assert_eq!(ok(r"return '\255'"), Value::from("\u{fffd}"));
}

#[test]
fn long_strings() {
    assert_eq!(ok("return [[\nline1\nline2]]"), Value::from("line1\nline2"));
    assert_eq!(ok("return [==[ ]] ]=] ]==]"), Value::from(" ]] ]=] "));
    assert_eq!(
        ok("return [[ -- not a comment ]]"),
        Value::from(" -- not a comment ")
    );
    assert_eq!(ok("return [[\r\nx]]"), Value::from("x"));
    assert!(parse("return [[ open").is_err());
    assert!(parse("--[[ open\nreturn {}").is_err());
}

#[test]
fn numerals() {
    assert_eq!(ok("return 0x1F"), Value::from(31i64));
    assert_eq!(ok("return 0XfF"), Value::from(255i64));
    assert_eq!(ok("return 1e3"), Value::from(1000i64));
    assert_eq!(ok("return .5"), Value::from(0.5));
    assert_eq!(ok("return 5."), Value::from(5i64));
    assert_eq!(ok("return -1.5E-3"), Value::from(-0.0015));
    assert_eq!(ok("return 1E+2"), Value::from(100i64));
    assert_eq!(ok("return 1.7918704836269e+15"), Value::from(1.7918704836269e15));
    let neg_zero = ok("return -0").as_f64().unwrap();
    assert_eq!(neg_zero.to_bits(), (-0.0f64).to_bits());
}

#[test]
fn infinities_and_nan_use_division_by_zero() {
    assert_eq!(ok("return 1/0"), Value::Number(f64::INFINITY));
    assert_eq!(ok("return -1/0"), Value::Number(f64::NEG_INFINITY));
    assert_eq!(ok("return 1/0.0"), Value::Number(f64::INFINITY));
    assert!(ok("return 0/0").as_f64().unwrap().is_nan());
    assert!(ok("return -0/0").as_f64().unwrap().is_nan());
    assert!(parse("return 1/2").is_err());
    assert!(parse("return 1/x").is_err());
    assert!(parse("return 1/").is_err());
}

#[test]
fn malformed_numerals_are_errors() {
    for src in [
        "return 1e",
        "return 0x",
        "return 3abc",
        "return 1..2",
        "return 1_000",
        "return 0x1G",
    ] {
        assert!(parse(src).is_err(), "{src} should be rejected");
    }
}

#[test]
fn code_is_rejected_with_a_position() {
    let e = err("return foo");
    assert_eq!((e.line, e.column), (1, 8));
    assert!(e.message.contains("names are not allowed"), "{e}");

    let e = err("return f(\"a\")");
    assert!(e.message.contains("function calls are not allowed"), "{e}");

    let e = err("return {\n  x = y\n}");
    assert_eq!((e.line, e.column), (2, 7));

    for src in [
        "return 1 + 2",
        "return print()",
        "return {[nil] = 1}",
        "return {[0/0] = 1}",
        "return {[{}] = 1}",
        "return {function() end}",
        "return {true false}",
        "return {x}",
        "return {a.b = 1}",
        "return {[1] 1}",
        "return",
        "return {} extra",
        "local x = 1 return {}",
        "return {",
        "return {1,,}",
        "return \"unterminated",
        "return \"line\nbreak\"",
        "",
        "-- only a comment",
        "{}",
    ] {
        assert!(parse(src).is_err(), "{src:?} should be rejected");
    }
}

#[test]
fn errors_report_line_and_column_in_characters() {
    // The name sits on line 2, after two multi-byte characters.
    let e = err("return {\n  ['ключ'] = x\n}");
    assert_eq!((e.line, e.column), (2, 14));
    assert_eq!(e.to_string(), format!("line 2, column 14: {}", e.message));
}

#[test]
fn unicode_text_round_trips() {
    let v = ok("return {['ключ'] = 'значение 🙂', [1] = '\\u00e9'}");
    assert_eq!(v.get("ключ"), &Value::from("значение 🙂"));
}

#[test]
fn crlf_line_endings_are_accepted() {
    let v = ok("-- x\r\nreturn {\r\n  a = 1,\r\n}\r\n");
    assert_eq!(v.get("a"), &Value::from(1i64));
}

#[test]
fn nesting_limit() {
    let at_limit = format!("return {}{}", "{".repeat(MAX_DEPTH), "}".repeat(MAX_DEPTH));
    assert!(parse(&at_limit).is_ok());
    let over = format!(
        "return {}{}",
        "{".repeat(MAX_DEPTH + 1),
        "}".repeat(MAX_DEPTH + 1)
    );
    let e = err(&over);
    assert!(e.message.contains("nested deeper"), "{e}");
    // Far past the limit still fails fast instead of overflowing the stack.
    let hostile = format!("return {}", "{".repeat(200_000));
    assert!(parse(&hostile).is_err());
}

#[test]
fn input_size_limit() {
    let mut big = String::with_capacity(MAX_INPUT_BYTES + 16);
    big.push_str("return ");
    big.extend(std::iter::repeat_n(' ', MAX_INPUT_BYTES));
    let e = err(&big);
    assert_eq!((e.line, e.column), (1, 1));
    assert!(e.message.contains("larger than"), "{e}");
}

#[test]
fn writer_refuses_values_past_the_nesting_limit() {
    let mut v = Value::from("leaf");
    for _ in 0..MAX_DEPTH {
        v = Value::array([v]);
    }
    let dir = scratch_dir("too-deep");
    let path = dir.join("deep.lua");
    // 200 tables is allowed.
    write_file(&path, &v).unwrap();
    assert!(same(&v, &parse(&fs::read_to_string(&path).unwrap()).unwrap()));
    let deeper = Value::array([v]);
    assert!(matches!(write_file(&path, &deeper), Err(WriteError::TooDeep)));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn duplicate_keys_in_a_table_constructor_keep_the_last() {
    assert_eq!(ok("return {[2] = 'a', [2] = 'b'}").get(2i64), &Value::from("b"));
}

#[test]
fn writer_output_for_a_nested_table() {
    let inner = table(vec![(Key::Int(1), Value::from(2i64))]);
    let outer = table(vec![(Key::from("x"), inner)]);
    assert_eq!(
        to_string(&outer),
        "return {\n    [\"x\"] = {\n        [1] = 2,\n    },\n}\n"
    );
}

#[test]
fn writer_orders_keys_like_ordered_pairs() {
    let v = table(vec![
        (Key::from("b"), Value::Bool(true)),
        (Key::Bool(true), Value::from(1i64)),
        (Key::from("B"), Value::from(2i64)),
        (Key::num(10.0).unwrap(), Value::from(3i64)),
        (Key::num(1.5).unwrap(), Value::from(4i64)),
        (Key::num(2.0).unwrap(), Value::from(5i64)),
        (Key::num(1.0).unwrap(), Value::from(6i64)),
    ]);
    let text = to_string(&v);
    let order: Vec<&str> = text.lines().skip(1).take(7).map(|l| l.trim()).collect();
    assert_eq!(
        order,
        vec![
            "[1] = 6,",
            "[1.5] = 4,",
            "[2] = 5,",
            "[10] = 3,",
            "[\"B\"] = 2,",
            "[\"b\"] = true,",
            "[true] = 1,",
        ]
    );
}

#[test]
fn writer_places_string_key_before_numeric_key_when_lua_string_sorts_first() {
    let value = table(vec![
        (Key::Int(2), Value::from("number")),
        (Key::from("!x"), Value::from("text")),
    ]);
    let text = to_string(&value);
    let lines: Vec<&str> = text.lines().skip(1).take(2).map(str::trim).collect();
    assert_eq!(lines, vec!["[\"!x\"] = \"text\",", "[2] = \"number\","]);
}

#[test]
fn writer_quotes_control_bytes_like_lua() {
    let v = Value::from("tab\there\u{7f}1\u{1}x\r\n\"\\");
    let text = to_string(&v);
    assert_eq!(text, "return \"tab\\9here\\1271\\1x\\13\\\n\\\"\\\\\"\n");
    assert!(same(&v, &parse(&text).unwrap()));
}

#[test]
fn write_file_replaces_the_file_and_leaves_no_temporary_file() {
    let dir = scratch_dir("write-file");
    let path = dir.join("metadata.epub.lua");
    fs::write(&path, "old contents").unwrap();
    let v = table(vec![(Key::from("doc_path"), Value::from("/books/a.epub"))]);
    write_file(&path, &v).unwrap();
    let text = fs::read_to_string(&path).unwrap();
    assert_eq!(
        text,
        format!(
            "-- {}\nreturn {{\n    [\"doc_path\"] = \"/books/a.epub\",\n}}\n",
            path.display()
        )
    );
    let names: Vec<String> = fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, vec!["metadata.epub.lua".to_owned()]);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn write_file_keeps_a_backup_of_an_old_file() {
    use std::fs::{File, FileTimes};
    use std::time::SystemTime;

    let dir = scratch_dir("write-file-old");
    let path = dir.join("metadata.lua");
    fs::write(&path, "old contents").unwrap();
    File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_times(FileTimes::new().set_modified(SystemTime::UNIX_EPOCH))
        .unwrap();
    write_file(&path, &Value::from("new")).unwrap();
    assert_eq!(
        fs::read_to_string(dir.join("metadata.lua.old")).unwrap(),
        "old contents"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn write_file_reports_io_errors() {
    let dir = scratch_dir("missing");
    let path = dir.join("no-such-dir").join("file.lua");
    assert!(matches!(write_file(&path, &Value::Nil), Err(WriteError::Io(_))));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn write_file_keeps_a_newline_out_of_the_header() {
    let dir = scratch_dir("header");
    let path = dir.join("odd\nname.lua");
    write_file(&path, &Value::Nil).unwrap();
    let text = fs::read_to_string(&path).unwrap();
    assert_eq!(text.lines().count(), 2);
    assert!(text.ends_with("return nil\n"));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn koreader_inf_and_nan_read_as_nil_like_luajit() {
    let v = kaia_luadata::parse("return { a = inf, b = -inf, c = nan, d = 1 }").unwrap();
    assert!(v.get("a").is_nil());
    assert!(v.get("b").is_nil());
    assert!(v.get("c").is_nil());
    assert_eq!(v.get("d").as_f64(), Some(1.0));
}
