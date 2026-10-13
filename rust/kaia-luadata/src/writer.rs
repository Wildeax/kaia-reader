//! Writes values in the layout of `frontend/dump.lua`, which KOReader reads
//! back with `loadfile`.

use std::cmp::Ordering;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::Path;

use kaia_events::{Key, Table, Value};

use crate::MAX_DEPTH;

/// Why [`write_file`] failed.
#[derive(Debug)]
pub enum WriteError {
    /// The value nests deeper than [`MAX_DEPTH`], so the parser would reject it.
    TooDeep,
    Io(io::Error),
}

impl fmt::Display for WriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WriteError::TooDeep => write!(f, "tables are nested deeper than {MAX_DEPTH} levels"),
            WriteError::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for WriteError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            WriteError::TooDeep => None,
            WriteError::Io(e) => Some(e),
        }
    }
}

impl From<io::Error> for WriteError {
    fn from(e: io::Error) -> Self {
        WriteError::Io(e)
    }
}

/// Serializes a value as a complete data file body: `return <expr>` and a
/// final newline. The output has no `-- path` header, see [`write_file`].
///
/// Tables are printed the way `dump.lua` prints them: one `[key] = value,`
/// line per entry, four spaces per level, keys in dump order. Numbers use
/// `%.14g` like `tostring` when that reproduces the value exactly. Otherwise
/// the shortest exact form is used, so the value survives a round trip.
pub fn to_string(value: &Value) -> String {
    let mut out = String::from("return ");
    write_value(&mut out, value, 0);
    out.push('\n');
    out
}

/// Writes a data file atomically: the contents go to a temporary file in the
/// same directory, which is fsynced and then renamed over `path`.
///
/// The file starts with `-- <path>`, the header that KOReader's
/// `util.writeToFile` writes. Values nested deeper than [`MAX_DEPTH`] are
/// refused, so every file written can be parsed again.
pub fn write_file(path: &Path, value: &Value) -> Result<(), WriteError> {
    if too_deep(value, 0) {
        return Err(WriteError::TooDeep);
    }
    let header: String = path
        .display()
        .to_string()
        .chars()
        .map(|c| if c == '\n' || c == '\r' { ' ' } else { c })
        .collect();
    let contents = format!("-- {header}\n{}", to_string(value));

    let dir = match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no file name"))?;
    let tmp = dir.join(format!(".{}.{}.tmp", name.to_string_lossy(), std::process::id()));

    let result =
        backup_old_file(path, dir).and_then(|()| write_and_rename(&tmp, path, dir, contents.as_bytes()));
    if result.is_err() {
        // Best effort: the temporary file may not exist if creating it failed.
        let _ = fs::remove_file(&tmp);
    }
    result.map_err(WriteError::Io)
}

fn backup_old_file(path: &Path, dir: &Path) -> io::Result<()> {
    let Ok(metadata) = fs::metadata(path) else {
        return Ok(());
    };
    let old_enough = metadata
        .modified()
        .ok()
        .and_then(|modified| modified.elapsed().ok())
        .is_some_and(|age| age.as_secs() > 60);
    if !old_enough {
        return Ok(());
    }
    let mut old_name = path.as_os_str().to_owned();
    old_name.push(".old");
    fs::rename(path, Path::new(&old_name))?;
    File::open(dir)?.sync_all()?;
    Ok(())
}

fn write_and_rename(tmp: &Path, path: &Path, dir: &Path, contents: &[u8]) -> io::Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(tmp)?;
    file.write_all(contents)?;
    file.sync_all()?;
    drop(file);
    fs::rename(tmp, path)?;
    // Persist the rename. Opening a directory for fsync works on Unix.
    File::open(dir)?.sync_all()
}

fn too_deep(value: &Value, level: usize) -> bool {
    match value {
        Value::Table(t) => level + 1 > MAX_DEPTH || t.values().any(|v| too_deep(v, level + 1)),
        _ => false,
    }
}

fn write_value(out: &mut String, value: &Value, indent: usize) {
    match value {
        Value::Nil => out.push_str("nil"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => out.push_str(&number(*n)),
        Value::Str(s) => quote_into(out, s),
        Value::Table(t) => write_table(out, t, indent),
    }
}

fn write_table(out: &mut String, table: &Table, indent: usize) {
    if table.is_empty() {
        out.push_str("{}");
        return;
    }
    let mut entries: Vec<(&Key, &Value)> = table.iter().collect();
    entries.sort_by(|a, b| key_order(a.0, b.0));
    out.push('{');
    for (key, value) in entries {
        out.push('\n');
        push_indent(out, indent + 1);
        out.push('[');
        write_key(out, key);
        out.push_str("] = ");
        write_value(out, value, indent + 1);
        out.push(',');
    }
    out.push('\n');
    push_indent(out, indent);
    out.push('}');
}

fn push_indent(out: &mut String, level: usize) {
    for _ in 0..level {
        out.push_str("    ");
    }
}

fn write_key(out: &mut String, key: &Key) {
    match key {
        Key::Int(i) => out.push_str(&number(*i as f64)),
        Key::Num(n) => out.push_str(&number(n.get())),
        Key::Str(s) => quote_into(out, s),
        Key::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
    }
}

/// The order `dump.lua` uses: keys of different kinds sort by their Lua string
/// form. Keys of the same kind use Lua's `<` ordering.
fn key_order(a: &Key, b: &Key) -> Ordering {
    fn lua_string(k: &Key) -> String {
        match k {
            Key::Int(i) => i.to_string(),
            Key::Num(n) => number(n.get()),
            Key::Str(s) => s.clone(),
            Key::Bool(b) => b.to_string(),
        }
    }
    match (a, b) {
        (Key::Int(x), Key::Int(y)) => x.cmp(y),
        (Key::Int(x), Key::Num(y)) => (*x as f64).total_cmp(&y.get()),
        (Key::Num(x), Key::Int(y)) => x.get().total_cmp(&(*y as f64)),
        (Key::Num(x), Key::Num(y)) => x.get().total_cmp(&y.get()),
        (Key::Str(x), Key::Str(y)) => x.as_bytes().cmp(y.as_bytes()),
        (Key::Bool(x), Key::Bool(y)) => x.cmp(y),
        _ => lua_string(a).as_bytes().cmp(lua_string(b).as_bytes()),
    }
}

/// Formats a number as a Lua numeral that reads back as the same bits.
pub(crate) fn number(n: f64) -> String {
    if n.is_nan() {
        return "0/0".to_owned();
    }
    if n.is_infinite() {
        return if n > 0.0 { "1/0" } else { "-1/0" }.to_owned();
    }
    let short = format_g14(n);
    if short
        .parse::<f64>()
        .is_ok_and(|back| back.to_bits() == n.to_bits())
    {
        short
    } else {
        format!("{n:e}")
    }
}

/// Formats like C's `%.14g`, which LuaJIT's `tostring` uses for numbers.
fn format_g14(n: f64) -> String {
    const PREC: i32 = 14;
    if n == 0.0 {
        return if n.is_sign_negative() { "-0" } else { "0" }.to_owned();
    }
    let sign = if n < 0.0 { "-" } else { "" };
    // `{:e}` rounds to PREC significant digits, and the exponent it reports is
    // the one after rounding, as in C.
    let sci = format!("{:.*e}", (PREC - 1) as usize, n.abs());
    let (mantissa, exp) = sci
        .split_once('e')
        .expect("`{:e}` output always contains an exponent");
    let exp: i32 = exp.parse().expect("`{:e}` exponent is an integer");
    if !(-4..PREC).contains(&exp) {
        let esign = if exp < 0 { '-' } else { '+' };
        format!("{sign}{}e{esign}{:02}", trim_fraction(mantissa), exp.abs())
    } else {
        let decimals = (PREC - 1 - exp) as usize;
        format!("{sign}{}", trim_fraction(&format!("{:.*}", decimals, n.abs())))
    }
}

fn trim_fraction(s: &str) -> String {
    if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_owned()
    } else {
        s.to_owned()
    }
}

/// Writes a string the way `string.format("%q")` does in LuaJIT: double quotes,
/// `\"`, `\\`, a backslash before each newline, and decimal escapes for control
/// bytes. A decimal escape is padded to three digits when a digit follows.
/// Other UTF-8 text is copied as is.
fn quote_into(out: &mut String, s: &str) {
    out.push('"');
    let bytes = s.as_bytes();
    for (i, ch) in s.char_indices() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\\n"),
            c if (c as u32) < 32 || c as u32 == 127 => {
                let code = c as u32;
                let next_is_digit = bytes.get(i + 1).is_some_and(u8::is_ascii_digit);
                let digits = if next_is_digit {
                    format!("{code:03}")
                } else {
                    code.to_string()
                };
                out.push('\\');
                out.push_str(&digits);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_match_tostring_when_exact() {
        assert_eq!(number(1.0), "1");
        assert_eq!(number(0.5), "0.5");
        assert_eq!(number(-5.0), "-5");
        assert_eq!(number(1e15), "1e+15");
        assert_eq!(number(1e-5), "1e-05");
        assert_eq!(number(0.1), "0.1");
        assert_eq!(number(-0.0), "-0");
        assert_eq!(number(1e14), "1e+14");
        assert_eq!(number(123456.0), "123456");
    }

    #[test]
    fn numbers_fall_back_to_exact_form() {
        // %.14g would give 9.007199254741e+15, which is not the same value.
        assert_eq!(number(9007199254740992.0), "9.007199254740992e15");
        assert_eq!(number(1.0 / 3.0), "3.333333333333333e-1");
    }

    #[test]
    fn special_numbers_use_division() {
        assert_eq!(number(f64::INFINITY), "1/0");
        assert_eq!(number(f64::NEG_INFINITY), "-1/0");
        assert_eq!(number(f64::NAN), "0/0");
    }

    #[test]
    fn strings_quote_like_lua_percent_q() {
        let mut out = String::new();
        quote_into(&mut out, "a\"b\\c\nd\te\0f\u{7f}\r9");
        assert_eq!(out, "\"a\\\"b\\\\c\\\nd\\9e\\0f\\127\\0139\"");
    }

    #[test]
    fn layout_matches_dump_lua() {
        let mut t = Table::new();
        t.insert(Key::from("b"), Value::from(true));
        t.insert(Key::from("a"), Value::Table(Table::new()));
        t.insert(Key::Int(1), Value::from(2.5));
        let text = to_string(&Value::Table(t));
        assert_eq!(
            text,
            "return {\n    [1] = 2.5,\n    [\"a\"] = {},\n    [\"b\"] = true,\n}\n"
        );
    }
}
