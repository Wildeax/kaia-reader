//! Cross-checks against LuaJIT, the runtime KOReader uses.
//!
//! Runs only when `KAIA_LUAJIT` names a luajit binary, for example the
//! emulator's `koreader-emulator-*/koreader/luajit`. A relative path is tried
//! from the test's directory, from `rust/`, and from the repository root.
//!
//! Two directions are checked:
//! 1. Files this crate writes are loaded with `dofile` in LuaJIT, and the
//!    values LuaJIT sees match the values written.
//! 2. Files that KOReader's `dump.lua` writes are parsed here, and the result
//!    matches the value LuaJIT loaded from the same file.

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use common::{arb_value, same, scratch_dir};
use kaia_events::{Key, Table, Value};
use kaia_luadata::{parse, write_file};
use proptest::strategy::{Strategy, ValueTree};
use proptest::test_runner::TestRunner;

/// Serializes whatever `dofile` returns, one tagged item per value, so this
/// crate can decode it without touching Lua's number or string formatting.
const CANON_SCRIPT: &str = r#"
local function emit(v, out)
    local t = type(v)
    if v == nil then out[#out + 1] = "N"
    elseif t == "boolean" then out[#out + 1] = v and "T" or "F"
    elseif t == "number" then out[#out + 1] = "n" .. string.format("%.17g", v) .. ";"
    elseif t == "string" then out[#out + 1] = "s" .. #v .. ":" .. v
    elseif t == "table" then
        local n = 0
        for _ in pairs(v) do n = n + 1 end
        out[#out + 1] = "t" .. n .. "{"
        for k, x in pairs(v) do emit(k, out); emit(x, out) end
        out[#out + 1] = "}"
    else
        error("unsupported type " .. t)
    end
end
local out = {}
emit(dofile(arg[1]), out)
io.write(table.concat(out))
"#;

/// Loads a file with `dump.lua` from the repository and writes the result
/// back out, so the output is exactly what KOReader would write.
const DUMP_SCRIPT: &str = r#"
package.path = arg[2] .. "/?.lua;" .. package.path
local dump = require("dump")
io.write(dump(dofile(arg[1])))
"#;

fn luajit() -> Option<PathBuf> {
    let raw = std::env::var_os("KAIA_LUAJIT")?;
    let given = PathBuf::from(&raw);
    if given.is_absolute() {
        assert!(given.is_file(), "KAIA_LUAJIT={} is not a file", given.display());
        return Some(given);
    }
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let candidates = [
        given.clone(),
        manifest.join("..").join(&given),
        manifest.join("../..").join(&given),
    ];
    let found = candidates.into_iter().find(|p| p.is_file());
    Some(found.unwrap_or_else(|| panic!("KAIA_LUAJIT={} is not a file", given.display())))
}

fn repo_frontend() -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../frontend");
    assert!(
        dir.join("dump.lua").is_file(),
        "{} has no dump.lua",
        dir.display()
    );
    dir.canonicalize().expect("frontend directory")
}

fn run_lua(bin: &Path, script: &Path, args: &[&str]) -> Vec<u8> {
    let out = Command::new(bin)
        .arg(script)
        .args(args)
        .output()
        .expect("run luajit");
    assert!(
        out.status.success(),
        "luajit failed on {}: {}",
        args.join(" "),
        String::from_utf8_lossy(&out.stderr)
    );
    out.stdout
}

/// Decodes the output of `CANON_SCRIPT`.
struct Canon<'a> {
    b: &'a [u8],
    p: usize,
}

impl Canon<'_> {
    fn take_until(&mut self, stop: u8) -> &[u8] {
        let start = self.p;
        while self.b[self.p] != stop {
            self.p += 1;
        }
        self.p += 1;
        &self.b[start..self.p - 1]
    }

    fn value(&mut self) -> Value {
        let tag = self.b[self.p];
        self.p += 1;
        match tag {
            b'N' => Value::Nil,
            b'T' => Value::Bool(true),
            b'F' => Value::Bool(false),
            b'n' => {
                let text = std::str::from_utf8(self.take_until(b';')).unwrap().to_owned();
                // %.17g prints NaN as "nan" or "-nan", which Rust does not parse.
                let n = if text.contains("nan") {
                    f64::NAN
                } else {
                    text.parse::<f64>()
                        .unwrap_or_else(|_| panic!("bad number {text}"))
                };
                Value::Number(n)
            }
            b's' => {
                let len: usize = std::str::from_utf8(self.take_until(b':'))
                    .unwrap()
                    .parse()
                    .unwrap();
                let s = String::from_utf8(self.b[self.p..self.p + len].to_vec()).expect("UTF-8 from LuaJIT");
                self.p += len;
                Value::Str(s)
            }
            b't' => {
                let n: usize = std::str::from_utf8(self.take_until(b'{'))
                    .unwrap()
                    .parse()
                    .unwrap();
                let mut table = Table::new();
                for _ in 0..n {
                    let key = match self.value() {
                        Value::Number(x) => Key::num(x).expect("no NaN keys"),
                        Value::Str(s) => Key::Str(s),
                        Value::Bool(b) => Key::Bool(b),
                        other => panic!("unexpected key {other:?}"),
                    };
                    let value = self.value();
                    table.insert(key, value);
                }
                assert_eq!(self.b[self.p], b'}');
                self.p += 1;
                Value::Table(table)
            }
            other => panic!("unknown tag {}", other as char),
        }
    }
}

fn decode_canon(bytes: &[u8]) -> Value {
    let mut c = Canon { b: bytes, p: 0 };
    let v = c.value();
    assert_eq!(c.p, bytes.len(), "trailing bytes from LuaJIT");
    v
}

#[test]
fn luajit_reads_what_the_writer_produces() {
    let Some(bin) = luajit() else {
        eprintln!("skipped: set KAIA_LUAJIT to a luajit binary to run this check");
        return;
    };
    let dir = scratch_dir("luajit-canon");
    let script = dir.join("canon.lua");
    fs::write(&script, CANON_SCRIPT).unwrap();

    let mut runner = TestRunner::deterministic();
    let strategy = arb_value();
    for i in 0..64 {
        let value = strategy.new_tree(&mut runner).unwrap().current();
        let file = dir.join(format!("value-{i}.lua"));
        write_file(&file, &value).unwrap();
        let seen = decode_canon(&run_lua(&bin, &script, &[file.to_str().unwrap()]));
        assert!(
            same(&value, &seen),
            "LuaJIT saw a different value for case {i}\nwritten:\n{}",
            fs::read_to_string(&file).unwrap()
        );
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn parser_reads_what_dump_lua_writes() {
    let Some(bin) = luajit() else {
        eprintln!("skipped: set KAIA_LUAJIT to a luajit binary to run this check");
        return;
    };
    let dir = scratch_dir("luajit-dump");
    let canon = dir.join("canon.lua");
    let dump = dir.join("dump.lua");
    fs::write(&canon, CANON_SCRIPT).unwrap();
    fs::write(&dump, DUMP_SCRIPT).unwrap();
    let frontend = repo_frontend();
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut checked = 0;
    for entry in fs::read_dir(&fixtures).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "lua") {
            continue;
        }
        let src = fs::read_to_string(&path).unwrap();
        let ours = parse(&src).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let dumped = run_lua(&bin, &dump, &[path.to_str().unwrap(), frontend.to_str().unwrap()]);
        let dumped = String::from_utf8(dumped).unwrap();
        let reparsed = parse(&format!("return {dumped}"))
            .unwrap_or_else(|e| panic!("{}: parse of dump.lua output: {e}\n{dumped}", path.display()));
        assert!(
            same(&ours, &reparsed),
            "{} differs after dump.lua",
            path.display()
        );
        // LuaJIT's own view agrees with ours.
        let lua_view = decode_canon(&run_lua(&bin, &canon, &[path.to_str().unwrap()]));
        assert!(
            same(&ours, &lua_view),
            "{} differs from LuaJIT's view",
            path.display()
        );
        checked += 1;
    }
    assert!(checked >= 5, "checked only {checked} fixtures");
    let _ = fs::remove_dir_all(&dir);
}

/// Prints `%q` for a string file, or `tostring` for each number line. For a
/// number, the second column says whether `tostring` reads back exactly.
const FORMAT_SCRIPT: &str = r#"
if arg[1] == "string" then
    io.write(string.format("%q", dofile(arg[2])))
else
    for line in io.lines(arg[2]) do
        local n = tonumber(line)
        io.write(tostring(n), "\t", (tonumber(tostring(n)) == n) and "1" or "0", "\n")
    end
end
"#;

#[test]
fn writer_formats_match_luajit_percent_q_and_tostring() {
    let Some(bin) = luajit() else {
        eprintln!("skipped: set KAIA_LUAJIT to a luajit binary to run this check");
        return;
    };
    let dir = scratch_dir("luajit-format");
    let script = dir.join("format.lua");
    fs::write(&script, FORMAT_SCRIPT).unwrap();

    // Strings: our text must equal `return ` plus LuaJIT's %q, byte for byte.
    let mut runner = TestRunner::deterministic();
    let strings = common::arb_string();
    for i in 0..64 {
        let s = strings.new_tree(&mut runner).unwrap().current();
        let file = dir.join(format!("str-{i}.lua"));
        let text = kaia_luadata::to_string(&Value::Str(s.clone()));
        fs::write(&file, &text).unwrap();
        let q = run_lua(&bin, &script, &["string", file.to_str().unwrap()]);
        let expected = format!("return {}\n", String::from_utf8(q).unwrap());
        assert_eq!(text, expected, "string {s:?}");
    }

    // Numbers: where tostring reads back exactly, our text must equal it.
    let mut numbers: Vec<f64> = vec![
        1e15,
        0.1,
        1.0 / 3.0,
        9007199254740992.0,
        1e-5,
        123456.0,
        -0.0,
        1e14,
        5e-324,
    ];
    let finite = proptest::prelude::any::<f64>().prop_filter("finite", |n| n.is_finite());
    for _ in 0..300 {
        numbers.push(finite.new_tree(&mut runner).unwrap().current());
    }
    // Short numbers, which %.14g reproduces exactly: integers and decimals with few digits.
    let short = proptest::prelude::any::<i64>().prop_map(|i| (i % 10_000_000_000_000) as f64);
    let decimal = proptest::prelude::any::<i32>().prop_map(|i| f64::from(i) / 1000.0);
    for _ in 0..100 {
        numbers.push(short.new_tree(&mut runner).unwrap().current());
        numbers.push(decimal.new_tree(&mut runner).unwrap().current());
    }
    let list = dir.join("numbers.txt");
    let text: String = numbers.iter().map(|n| format!("{n:e}\n")).collect();
    fs::write(&list, text).unwrap();
    let out = run_lua(&bin, &script, &["number", list.to_str().unwrap()]);
    let out = String::from_utf8(out).unwrap();
    let mut exact = 0;
    for (n, line) in numbers.iter().zip(out.lines()) {
        let (lua_text, flag) = line.split_once('\t').expect("tab-separated");
        if flag == "1" {
            exact += 1;
            let ours = kaia_luadata::to_string(&Value::Number(*n));
            assert_eq!(ours, format!("return {lua_text}\n"), "number {n:e}");
        }
    }
    assert!(exact > 150, "only {exact} numbers were exact in tostring");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn luajit_loads_the_deepest_file_the_writer_accepts() {
    let Some(bin) = luajit() else {
        eprintln!("skipped: set KAIA_LUAJIT to a luajit binary to run this check");
        return;
    };
    let dir = scratch_dir("luajit-depth");
    let script = dir.join("canon.lua");
    fs::write(&script, CANON_SCRIPT).unwrap();
    let mut v = Value::from("leaf");
    for _ in 0..kaia_luadata::MAX_DEPTH {
        let mut t = Table::new();
        t.insert(Key::from("k"), v);
        v = Value::Table(t);
    }
    let file = dir.join("deep.lua");
    write_file(&file, &v).unwrap();
    let seen = decode_canon(&run_lua(&bin, &script, &[file.to_str().unwrap()]));
    assert!(same(&v, &seen), "LuaJIT did not load the deepest accepted file");
    let _ = fs::remove_dir_all(&dir);
}
