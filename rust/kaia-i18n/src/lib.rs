//! Gettext catalog loading and lookup for the Kaia UI.
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Entry {
    pub context: Option<String>,
    pub id: String,
    pub plural: Option<String>,
    pub forms: Vec<String>,
}
#[derive(Clone, Debug, Default)]
pub struct Catalog {
    entries: HashMap<(Option<String>, String), Entry>,
    plural: PluralRule,
}
#[derive(Clone, Debug)]
struct PluralRule {
    count: usize,
    expr: Expr,
}
#[derive(Clone, Debug)]
enum Expr {
    Num(i64),
    N,
    Unary(Box<Expr>),
    Bin(String, Box<Expr>, Box<Expr>),
    Cond(Box<Expr>, Box<Expr>, Box<Expr>),
}
impl Default for PluralRule {
    fn default() -> Self {
        Self {
            count: 2,
            expr: Expr::Bin("!=".into(), Box::new(Expr::N), Box::new(Expr::Num(1))),
        }
    }
}

fn unquote(s: &str) -> Result<String, String> {
    let s = s.trim();
    if !s.starts_with('"') || !s.ends_with('"') {
        return Err("expected quoted string".into());
    }
    let mut out = String::new();
    let mut chars = s[1..s.len() - 1].chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next().ok_or("trailing escape")? {
            'n' => out.push('\n'),
            'r' => out.push('\r'),
            't' => out.push('\t'),
            '"' => out.push('"'),
            '\\' => out.push('\\'),
            x => {
                out.push('\\');
                out.push(x)
            }
        }
    }
    Ok(out)
}
/// Parse gettext PO entries. Fuzzy entries and comments are ignored.
pub fn parse_po(input: &str) -> Result<Vec<Entry>, String> {
    let mut entries = Vec::new();
    let mut fields: HashMap<String, String> = HashMap::new();
    let mut fuzzy = false;
    let mut active: Option<String> = None;
    let finish = |fields: &mut HashMap<String, String>, fuzzy: &mut bool, entries: &mut Vec<Entry>| {
        if !fields.is_empty() && !*fuzzy {
            let id = fields.get("msgid").cloned().unwrap_or_default();
            let max = fields
                .keys()
                .filter_map(|k| {
                    k.strip_prefix("msgstr[")
                        .and_then(|x| x.strip_suffix(']'))
                        .and_then(|x| x.parse::<usize>().ok())
                })
                .max()
                .unwrap_or(0);
            let forms = if let Some(value) = fields.get("msgstr") {
                vec![value.clone()]
            } else {
                (0..=max)
                    .map(|i| fields.get(&format!("msgstr[{i}]")).cloned().unwrap_or_default())
                    .collect()
            };
            entries.push(Entry {
                context: fields.get("msgctxt").cloned(),
                id,
                plural: fields.get("msgid_plural").cloned(),
                forms,
            });
        }
        fields.clear();
        *fuzzy = false;
    };
    for raw in input.lines() {
        let line = raw.trim();
        if line.is_empty() {
            finish(&mut fields, &mut fuzzy, &mut entries);
            active = None;
            continue;
        }
        if line.starts_with('#') {
            if line.starts_with("#, ") || line.starts_with("#,") {
                fuzzy = line[2..].split(',').any(|x| x.trim() == "fuzzy");
            }
            continue;
        }
        if line.starts_with('"') {
            let key = active.as_ref().ok_or("continuation without field")?.clone();
            let value = unquote(line)?;
            fields.entry(key).or_default().push_str(&value);
            continue;
        }
        let (key, val) = line
            .split_once(char::is_whitespace)
            .ok_or_else(|| format!("invalid PO line: {line}"))?;
        let value = unquote(val)?;
        active = Some(key.to_string());
        fields.insert(key.to_string(), value);
    }
    finish(&mut fields, &mut fuzzy, &mut entries);
    Ok(entries)
}

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Number(i64),
    N,
    Op(String),
    L,
    R,
    Q,
    Colon,
    End,
}
fn tokens(s: &str) -> Result<Vec<Tok>, String> {
    let b = s.as_bytes();
    let (mut i, mut v) = (0, Vec::new());
    while i < b.len() {
        if b[i].is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if b[i].is_ascii_digit() {
            let st = i;
            while i < b.len() && b[i].is_ascii_digit() {
                i += 1
            }
            v.push(Tok::Number(s[st..i].parse().map_err(|_| "number overflow")?));
            continue;
        }
        if b[i] == b'n' {
            v.push(Tok::N);
            i += 1;
            continue;
        }
        let c = b[i] as char;
        if c == '(' {
            v.push(Tok::L);
            i += 1;
            continue;
        }
        if c == ')' {
            v.push(Tok::R);
            i += 1;
            continue;
        }
        if c == '?' {
            v.push(Tok::Q);
            i += 1;
            continue;
        }
        if c == ':' {
            v.push(Tok::Colon);
            i += 1;
            continue;
        }
        let two = if i + 1 < b.len() { &s[i..i + 2] } else { "" };
        if ["&&", "||", "==", "!=", "<=", ">="].contains(&two) {
            v.push(Tok::Op(two.into()));
            i += 2;
            continue;
        }
        if "+-*%<>!".contains(c) {
            v.push(Tok::Op(c.to_string()));
            i += 1;
            continue;
        }
        return Err(format!("invalid plural expression character {c}"));
    }
    v.push(Tok::End);
    Ok(v)
}
struct Parser {
    t: Vec<Tok>,
    i: usize,
}
impl Parser {
    fn peek(&self) -> &Tok {
        &self.t[self.i]
    }
    fn take(&mut self) -> Tok {
        let x = self.t[self.i].clone();
        self.i += 1;
        x
    }
    fn atom(&mut self) -> Result<Expr, String> {
        match self.take() {
            Tok::Number(n) => Ok(Expr::Num(n)),
            Tok::N => Ok(Expr::N),
            Tok::L => {
                let x = self.conditional()?;
                if self.take() != Tok::R {
                    return Err("missing )".into());
                }
                Ok(x)
            }
            Tok::Op(x) if x == "!" || x == "-" => Ok(Expr::Unary(Box::new(self.atom()?))),
            _ => Err("expected expression".into()),
        }
    }
    fn binary(&mut self, min: u8) -> Result<Expr, String> {
        let mut lhs = self.atom()?;
        while let Tok::Op(value) = self.peek() {
            let op = value.clone();
            let p = match op.as_str() {
                "||" => 1,
                "&&" => 2,
                "==" | "!=" | "<" | ">" | "<=" | ">=" => 3,
                "+" | "-" => 4,
                "*" | "%" => 5,
                _ => break,
            };
            if p < min {
                break;
            }
            self.take();
            let rhs = self.binary(p + 1)?;
            lhs = Expr::Bin(op, Box::new(lhs), Box::new(rhs))
        }
        Ok(lhs)
    }
    fn conditional(&mut self) -> Result<Expr, String> {
        let c = self.binary(1)?;
        if self.peek() == &Tok::Q {
            self.take();
            let a = self.conditional()?;
            if self.take() != Tok::Colon {
                return Err("missing :".into());
            }
            let b = self.conditional()?;
            Ok(Expr::Cond(Box::new(c), Box::new(a), Box::new(b)))
        } else {
            Ok(c)
        }
    }
}
fn truth(x: i64) -> bool {
    x != 0
}
impl Expr {
    fn eval(&self, n: i64) -> i64 {
        match self {
            Self::Num(x) => *x,
            Self::N => n,
            Self::Unary(x) => -x.eval(n),
            Self::Cond(c, a, b) => {
                if truth(c.eval(n)) {
                    a.eval(n)
                } else {
                    b.eval(n)
                }
            }
            Self::Bin(op, a, b) => {
                let x = a.eval(n);
                if op == "&&" && !truth(x) {
                    return 0;
                }
                if op == "||" && truth(x) {
                    return 1;
                }
                let y = b.eval(n);
                match op.as_str() {
                    "||" => i64::from(truth(y)),
                    "&&" => i64::from(truth(y)),
                    "==" => i64::from(x == y),
                    "!=" => i64::from(x != y),
                    "<" => i64::from(x < y),
                    ">" => i64::from(x > y),
                    "<=" => i64::from(x <= y),
                    ">=" => i64::from(x >= y),
                    "+" => x.saturating_add(y),
                    "-" => x.saturating_sub(y),
                    "*" => x.saturating_mul(y),
                    "%" => {
                        if y == 0 {
                            0
                        } else {
                            x % y
                        }
                    }
                    _ => 0,
                }
            }
        }
    }
}
pub fn plural_index(expression: &str, n: i64) -> Result<usize, String> {
    let mut p = Parser {
        t: tokens(expression)?,
        i: 0,
    };
    let e = p.conditional()?;
    if p.peek() != &Tok::End {
        return Err("trailing plural expression tokens".into());
    }
    Ok(e.eval(n).max(0) as usize)
}
fn parse_rule(po: &str) -> PluralRule {
    let header = parse_po(po)
        .map(|entries| header_of(&entries))
        .unwrap_or_default();
    rule_from_header(&header)
}

/// The msgstr of a catalog's header entry, the one with the empty msgid. Empty when there is none.
fn header_of(entries: &[Entry]) -> String {
    entries
        .iter()
        .find(|e| e.id.is_empty())
        .and_then(|e| e.forms.first())
        .cloned()
        .unwrap_or_default()
}

/// The plural rule that a header declares with `Plural-Forms`. English when it declares none.
fn rule_from_header(hdr: &str) -> PluralRule {
    let mut rule = PluralRule::default();
    for line in hdr.lines() {
        if let Some(x) = line.strip_prefix("Plural-Forms:") {
            for part in x.split(';') {
                let part = part.trim();
                if let Some(v) = part.strip_prefix("nplurals=") {
                    rule.count = v.parse().unwrap_or(2)
                }
                if let Some(v) = part.strip_prefix("plural=") {
                    if let Ok(t) = tokens(v.trim()) {
                        let mut p = Parser { t, i: 0 };
                        if let Ok(expr) = p.conditional() {
                            rule.expr = expr
                        }
                    }
                }
            }
        }
    }
    rule.count = rule.count.max(1);
    rule
}

/// Whether `root` holds a catalog for the tag `lang`: KOReader's `l10n/<lang>` or Kaia's
/// `l10n-kaia/<lang>`, each as a `.po` or a `.mo` file.
pub fn has_catalog(root: &Path, lang: &str) -> bool {
    catalog_file(&root.join("l10n").join(lang), "koreader").is_some()
        || catalog_file(&root.join("l10n-kaia").join(lang), "kaia").is_some()
}

/// The catalog file `stem` in `dir`. The `.po` file wins when both exist, else the `.mo` file.
fn catalog_file(dir: &Path, stem: &str) -> Option<PathBuf> {
    let po = dir.join(format!("{stem}.po"));
    if po.is_file() {
        return Some(po);
    }
    let mo = dir.join(format!("{stem}.mo"));
    mo.is_file().then_some(mo)
}

/// The entries of one catalog file. A `.mo` file is read as compiled, any other file as PO text.
fn read_catalog(path: &Path) -> Result<Vec<Entry>, String> {
    let bytes = fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let parsed = if path.extension().and_then(|ext| ext.to_str()) == Some("mo") {
        parse_mo(&bytes)
    } else {
        let text = std::str::from_utf8(&bytes).map_err(|e| format!("not UTF-8: {e}"))?;
        parse_po(text)
    };
    parsed.map_err(|e| format!("{}: {e}", path.display()))
}

/// Parses a compiled GNU gettext catalog (`.mo`), in either byte order. The entries have the
/// shape [`parse_po`] gives: the msgctxt is the part of the original before the EOT byte
/// (0x04), a plural msgid follows a NUL byte, and the msgstr forms are the NUL-separated
/// parts of the translation. The header entry is kept, as in `parse_po`.
pub fn parse_mo(bytes: &[u8]) -> Result<Vec<Entry>, String> {
    let little = match bytes.get(..4) {
        Some([0xde, 0x12, 0x04, 0x95]) => true,
        Some([0x95, 0x04, 0x12, 0xde]) => false,
        Some(_) => return Err("not a GNU MO file: bad magic number".into()),
        None => return Err("not a GNU MO file: too short".into()),
    };
    let mo = Mo { bytes, little };
    let count = mo.word(8)?;
    let originals = mo.word(12)?;
    let translations = mo.word(16)?;
    let mut entries = Vec::with_capacity(count.min(bytes.len() / 16));
    for index in 0..count {
        let original = text(mo.string(originals, index)?)?;
        let translated = text(mo.string(translations, index)?)?;
        let (context, rest) = match original.split_once('\u{4}') {
            Some((context, rest)) => (Some(context.to_string()), rest.to_string()),
            None => (None, original.clone()),
        };
        let (id, plural) = match rest.split_once('\0') {
            Some((id, plural)) => (id.to_string(), Some(plural.to_string())),
            None => (rest.clone(), None),
        };
        let forms = translated.split('\0').map(str::to_string).collect();
        entries.push(Entry {
            context,
            id,
            plural,
            forms,
        });
    }
    Ok(entries)
}

/// The words and strings of a `.mo` file, read in the byte order of the file.
struct Mo<'a> {
    bytes: &'a [u8],
    little: bool,
}

impl Mo<'_> {
    /// The 32-bit word at byte `at`.
    fn word(&self, at: usize) -> Result<usize, String> {
        let raw: [u8; 4] = at
            .checked_add(4)
            .and_then(|end| self.bytes.get(at..end))
            .and_then(|raw| raw.try_into().ok())
            .ok_or_else(truncated)?;
        let value = if self.little {
            u32::from_le_bytes(raw)
        } else {
            u32::from_be_bytes(raw)
        };
        Ok(value as usize)
    }

    /// Row `index` of the table at byte `table` gives the length and the offset of a string.
    fn string(&self, table: usize, index: usize) -> Result<&[u8], String> {
        let row = index
            .checked_mul(8)
            .and_then(|offset| table.checked_add(offset))
            .ok_or_else(truncated)?;
        let len = self.word(row)?;
        let start = self.word(row.checked_add(4).ok_or_else(truncated)?)?;
        let end = start.checked_add(len).ok_or_else(truncated)?;
        self.bytes
            .get(start..end)
            .ok_or_else(|| "MO string lies outside the file".to_string())
    }
}

fn truncated() -> String {
    "MO file is truncated".to_string()
}

/// The text of a `.mo` string, which must be UTF-8.
fn text(raw: &[u8]) -> Result<String, String> {
    std::str::from_utf8(raw)
        .map(str::to_string)
        .map_err(|e| format!("MO string is not UTF-8: {e}"))
}
impl Catalog {
    /// Load the catalogs for `lang` beneath `dir`: Kaia's `l10n-kaia/<tag>/kaia.po` and
    /// KOReader's `l10n/<tag>/koreader.po`. Each is read from its `.po` file or, when there
    /// is none, from the `.mo` file beside it. A regional tag falls back to its base
    /// language. An entry with no translation does not hide the next catalog's translation.
    pub fn load(dir: impl AsRef<Path>, lang: &str) -> Result<Self, String> {
        let root = dir.as_ref();
        let normalized = lang.split(['.', '@']).next().unwrap_or(lang).replace('-', "_");
        let mut tags = vec![normalized.clone()];
        if let Some((base, _)) = normalized.split_once('_') {
            tags.push(base.to_string())
        }
        let mut cat = Catalog::default();
        let mut first_header = None;
        for tag in tags {
            for (folder, stem) in [("l10n-kaia", "kaia"), ("l10n", "koreader")] {
                let Some(source) = catalog_file(&root.join(folder).join(&tag), stem) else {
                    continue;
                };
                let entries = read_catalog(&source)?;
                if first_header.is_none() {
                    first_header = Some(header_of(&entries))
                }
                for e in entries {
                    if !e.id.is_empty() && e.forms.iter().any(|form| !form.is_empty()) {
                        cat.entries.entry((e.context.clone(), e.id.clone())).or_insert(e);
                    }
                }
            }
        }
        if let Some(header) = first_header {
            cat.plural = rule_from_header(&header)
        }
        Ok(cat)
    }
    /// Build a catalog from the bytes of a compiled `.mo` file.
    pub fn from_mo(bytes: &[u8]) -> Result<Self, String> {
        let entries = parse_mo(bytes)?;
        let mut c = Catalog {
            plural: rule_from_header(&header_of(&entries)),
            ..Self::default()
        };
        for e in entries {
            if !e.id.is_empty() {
                c.entries.insert((e.context.clone(), e.id.clone()), e);
            }
        }
        Ok(c)
    }
    pub fn from_po(po: &str) -> Result<Self, String> {
        let mut c = Self::default();
        for e in parse_po(po)? {
            if !e.id.is_empty() {
                c.entries.insert((e.context.clone(), e.id.clone()), e);
            }
        }
        c.plural = parse_rule(po);
        Ok(c)
    }
    pub fn translate(&self, id: &str) -> String {
        self.lookup(None, id).unwrap_or_else(|| id.to_string())
    }
    pub fn translate_ctx(&self, ctx: &str, id: &str) -> String {
        self.lookup(Some(ctx), id).unwrap_or_else(|| id.to_string())
    }
    fn lookup(&self, ctx: Option<&str>, id: &str) -> Option<String> {
        self.entries
            .get(&(ctx.map(str::to_string), id.to_string()))
            .and_then(|e| e.forms.first())
            .filter(|s| !s.is_empty())
            .cloned()
    }
    pub fn translate_n(&self, one: &str, plural: &str, n: i64) -> String {
        let Some(e) = self.entries.get(&(None, one.to_string())) else {
            return if n == 1 {
                one.to_string()
            } else {
                plural.to_string()
            };
        };
        let i = if e.plural.is_some() {
            self.plural.expr.eval(n).max(0) as usize
        } else {
            0
        };
        e.forms
            .get(i)
            .filter(|s| !s.is_empty())
            .cloned()
            .unwrap_or_else(|| {
                if n == 1 {
                    one.to_string()
                } else {
                    plural.to_string()
                }
            })
    }
}
// UI lookups use thread-local state so tests and independent UI threads do not race.
thread_local! { static CURRENT: std::cell::RefCell<Catalog> = std::cell::RefCell::new(Catalog::default()); }
pub fn set_current(catalog: Catalog) {
    CURRENT.with(|c| *c.borrow_mut() = catalog)
}
pub fn tr(id: &str) -> String {
    CURRENT.with(|c| c.borrow().translate(id))
}
pub fn tr_ctx(ctx: &str, id: &str) -> String {
    CURRENT.with(|c| c.borrow().translate_ctx(ctx, id))
}
pub fn tr_n(one: &str, plural: &str, n: i64) -> String {
    CURRENT.with(|c| c.borrow().translate_n(one, plural, n))
}
/// Replace `%1`, `%2`, and later numbered slots using the matching argument.
pub fn format(template: &str, args: &[&str]) -> String {
    let mut out = String::new();
    let b = template.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 1 < b.len() && b[i + 1].is_ascii_digit() {
            let mut j = i + 1;
            while j < b.len() && b[j].is_ascii_digit() {
                j += 1
            }
            if let Ok(slot) = template[i + 1..j].parse::<usize>() {
                if let Some(value) = slot.checked_sub(1).and_then(|x| args.get(x)) {
                    out.push_str(value);
                    i = j;
                    continue;
                }
            }
        }
        let ch = template[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8()
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_fields_multiline_escapes_and_fuzzy() {
        let p="msgid \"\"\nmsgstr \"Plural-Forms: nplurals=2; plural=(n != 1);\\n\"\n\n#. note\nmsgctxt \"menu\\nlabel\"\nmsgid \"Open\"\nmsgstr \"Ouvrir\\nnow\"\n\n#, fuzzy\nmsgid \"Skip\"\nmsgstr \"Ignorer\"\n\nmsgid \"one\"\nmsgid_plural \"many\"\nmsgstr[0] \"un\"\nmsgstr[1] \"plusieurs\"\n";
        let e = parse_po(p).unwrap();
        assert_eq!(e.len(), 3);
        assert_eq!(e[1].context.as_deref(), Some("menu\nlabel"));
        assert_eq!(e[1].forms[0], "Ouvrir\nnow");
        assert_eq!(e[2].forms[1], "plusieurs");
    }
    #[test]
    fn real_plural_rules() {
        let cases=[("n%10==1 && n%100!=11 ? 0 : n%10>=2 && n%10<=4 && (n%100<12 || n%100>14) ? 1 : n%10==0 || n%10>=5 || n%100>=11 && n%100<=14 ? 2 : 3",vec![(1,0),(2,1),(5,2),(21,0)]),("n==1 ? 0 : n%10>=2 && n%10<=4 && (n%100<12 || n%100>14) ? 1 : 2",vec![(1,0),(3,1),(12,2)]),("n==0 ? 0 : n==1 ? 1 : n==2 ? 2 : n%100>=3 && n%100<=10 ? 3 : n%100>=11 ? 4 : 5",vec![(0,0),(1,1),(2,2),(7,3),(15,4),(100,5)]),("(n==1) ? 0 : (n>=2 && n<=4) ? 1 : 2",vec![(1,0),(3,1),(8,2)]),("n > 1",vec![(0,0),(1,0),(2,1)]),("0",vec![(1,0),(99,0)])];
        for (e, ns) in cases {
            for (n, want) in ns {
                assert_eq!(plural_index(e, n).unwrap(), want, "{e} at {n}")
            }
        }
    }
    #[test]
    fn parses_plural_headers_from_koreader_catalogs() {
        let catalogs = [
            ("ru", include_str!("../../../l10n/ru/koreader.po")),
            ("pl", include_str!("../../../l10n/pl/koreader.po")),
            ("ar", include_str!("../../../l10n/ar/koreader.po")),
            ("cs", include_str!("../../../l10n/cs/koreader.po")),
            ("fr", include_str!("../../../l10n/fr/koreader.po")),
            ("ja", include_str!("../../../l10n/ja/koreader.po")),
        ];
        let expected_forms = [("ru", 4), ("pl", 4), ("ar", 6), ("cs", 3), ("fr", 2), ("ja", 1)];
        for (lang, po) in catalogs {
            let rule = parse_rule(po);
            assert_eq!(
                rule.count,
                expected_forms.iter().find(|x| x.0 == lang).unwrap().1,
                "{lang}"
            );
            for n in [0, 1, 2, 3, 5, 11, 21, 100] {
                assert!(
                    rule.expr.eval(n) >= 0 && (rule.expr.eval(n) as usize) < rule.count,
                    "{lang} n={n}"
                );
            }
        }
    }
    #[test]
    fn catalog_context_and_plural() {
        let p="msgid \"\"\nmsgstr \"Plural-Forms: nplurals=2; plural=n != 1;\\n\"\n\nmsgid \"book\"\nmsgid_plural \"%1 books\"\nmsgstr[0] \"livre\"\nmsgstr[1] \"livres\"\n\nmsgctxt \"verb\"\nmsgid \"Open\"\nmsgstr \"Ouvrir\"\n";
        let c = Catalog::from_po(p).unwrap();
        assert_eq!(c.translate_ctx("verb", "Open"), "Ouvrir");
        assert_eq!(c.translate_n("book", "%1 books", 2), "livres")
    }
    #[test]
    fn path_fallback_and_precedence() {
        let base = std::env::temp_dir().join(format!("kaia-i18n-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(base.join("l10n-kaia/pt")).unwrap();
        fs::create_dir_all(base.join("l10n/pt")).unwrap();
        fs::write(
            base.join("l10n-kaia/pt/kaia.po"),
            "msgid \"Hello\"\nmsgstr \"Oi Kaia\"\n",
        )
        .unwrap();
        fs::write(
            base.join("l10n/pt/koreader.po"),
            "msgid \"Hello\"\nmsgstr \"Oi KOReader\"\n\nmsgid \"Open\"\nmsgstr \"Abrir\"\n",
        )
        .unwrap();
        let c = Catalog::load(&base, "pt_BR").unwrap();
        assert_eq!(c.translate("Hello"), "Oi Kaia");
        assert_eq!(c.translate("Open"), "Abrir");
        let _ = fs::remove_dir_all(base);
    }
    #[test]
    fn formats_slots() {
        assert_eq!(format("%1 of %2 books", &["3", "8"]), "3 of 8 books")
    }
}

#[cfg(test)]
mod mo_tests {
    use super::*;

    const PO: &str = "\
msgid \"\"
msgstr \"\"
\"Content-Type: text/plain; charset=UTF-8\\n\"
\"Plural-Forms: nplurals=2; plural=n > 1;\\n\"

msgid \"Open\"
msgstr \"Ouvrir\"

msgctxt \"verb\"
msgid \"Open\"
msgstr \"Ouvrir (verbe)\"

msgid \"%1 book\"
msgid_plural \"%1 books\"
msgstr[0] \"%1 livre\"
msgstr[1] \"%1 livres\"

msgctxt \"menu\"
msgid \"Settings\"
msgid_plural \"Settings\"
msgstr[0] \"Réglage\"
msgstr[1] \"Réglages\"

#, fuzzy
msgid \"Skip\"
msgstr \"Ignorer\"
";

    /// Compiles entries the way msgfmt lays out a `.mo` file, in either byte order.
    fn compile_mo(entries: &[Entry], little: bool) -> Vec<u8> {
        let word = |n: usize| -> [u8; 4] {
            let n = n as u32;
            if little {
                n.to_le_bytes()
            } else {
                n.to_be_bytes()
            }
        };
        let mut pairs: Vec<(Vec<u8>, Vec<u8>)> = entries
            .iter()
            .map(|e| {
                let mut original = Vec::new();
                if let Some(context) = &e.context {
                    original.extend_from_slice(context.as_bytes());
                    original.push(0x04);
                }
                original.extend_from_slice(e.id.as_bytes());
                if let Some(plural) = &e.plural {
                    original.push(0);
                    original.extend_from_slice(plural.as_bytes());
                }
                (original, e.forms.join("\0").into_bytes())
            })
            .collect();
        pairs.sort();
        let count = pairs.len();
        let (originals_at, translations_at) = (28, 28 + count * 8);
        let mut offset = translations_at + count * 8;
        let (mut originals, mut translations, mut strings) = (Vec::new(), Vec::new(), Vec::new());
        for (original, _) in &pairs {
            originals.extend(word(original.len()));
            originals.extend(word(offset));
            strings.extend_from_slice(original);
            strings.push(0);
            offset += original.len() + 1;
        }
        for (_, translated) in &pairs {
            translations.extend(word(translated.len()));
            translations.extend(word(offset));
            strings.extend_from_slice(translated);
            strings.push(0);
            offset += translated.len() + 1;
        }
        let mut mo = Vec::new();
        for field in [0x9504_12de, 0, count, originals_at, translations_at, 0, 0] {
            mo.extend(word(field));
        }
        mo.extend(originals);
        mo.extend(translations);
        mo.extend(strings);
        mo
    }

    fn sorted(mut entries: Vec<Entry>) -> Vec<Entry> {
        entries.sort_by(|a, b| (&a.context, &a.id).cmp(&(&b.context, &b.id)));
        entries
    }

    fn put(base: &Path, rel: &str, bytes: &[u8]) {
        let file = base.join(rel);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, bytes).unwrap();
    }

    #[test]
    fn mo_and_po_load_the_same_entries() {
        let from_po = parse_po(PO).unwrap();
        assert!(from_po
            .iter()
            .any(|e| e.context.as_deref() == Some("menu") && e.plural.is_some()));
        assert!(
            from_po.iter().all(|e| e.id != "Skip"),
            "a fuzzy entry is not loaded"
        );
        for little in [true, false] {
            let from_mo = parse_mo(&compile_mo(&from_po, little)).unwrap();
            assert_eq!(
                sorted(from_mo),
                sorted(from_po.clone()),
                "little endian: {little}"
            );
        }
    }

    #[test]
    fn msgfmt_output_loads_the_same_entries() {
        if std::process::Command::new("msgfmt")
            .arg("--version")
            .output()
            .is_err()
        {
            eprintln!("skipped: msgfmt is not installed");
            return;
        }
        let dir = std::env::temp_dir().join(format!("kaia-i18n-msgfmt-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let po = dir.join("fixture.po");
        fs::write(&po, PO).unwrap();
        for endianness in ["little", "big"] {
            let mo = dir.join(format!("{endianness}.mo"));
            let status = std::process::Command::new("msgfmt")
                .arg(format!("--endianness={endianness}"))
                .arg("-o")
                .arg(&mo)
                .arg(&po)
                .status()
                .unwrap();
            assert!(status.success(), "msgfmt --endianness={endianness}");
            let compiled = parse_mo(&fs::read(&mo).unwrap()).unwrap();
            assert_eq!(sorted(compiled), sorted(parse_po(PO).unwrap()), "{endianness}");
        }
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn koreader_fr_mo_holds_the_translations_of_its_po() {
        // msgfmt drops POT-Creation-Date from the header, so only the plural rule is compared.
        let plural_rule = |entries: &[Entry]| -> Vec<String> {
            let header = header_of(entries);
            header
                .lines()
                .filter(|line| line.starts_with("Plural-Forms:"))
                .map(str::to_string)
                .collect()
        };
        let translations = |entries: Vec<Entry>| -> Vec<Entry> {
            sorted(
                entries
                    .into_iter()
                    .filter(|e| !e.id.is_empty() && e.forms.iter().all(|form| !form.is_empty()))
                    .collect(),
            )
        };
        let po_entries = parse_po(include_str!("../../../l10n/fr/koreader.po")).unwrap();
        let mo_entries = parse_mo(include_bytes!("../../../l10n/fr/koreader.mo")).unwrap();
        assert_eq!(plural_rule(&mo_entries), plural_rule(&po_entries));
        assert_eq!(
            plural_rule(&mo_entries),
            ["Plural-Forms: nplurals=2; plural=n > 1;"]
        );
        let (po, mo) = (translations(po_entries), translations(mo_entries));
        assert!(mo.len() > 3000, "{} translations", mo.len());
        assert_eq!(mo, po);
    }

    #[test]
    fn mo_header_gives_the_plural_rule_and_the_contexts() {
        let catalog = Catalog::from_mo(&compile_mo(&parse_po(PO).unwrap(), false)).unwrap();
        assert_eq!(catalog.translate_ctx("verb", "Open"), "Ouvrir (verbe)");
        assert_eq!(catalog.translate_ctx("menu", "Settings"), "Réglage");
        assert_eq!(catalog.translate_n("%1 book", "%1 books", 1), "%1 livre");
        assert_eq!(catalog.translate_n("%1 book", "%1 books", 2), "%1 livres");
    }

    #[test]
    fn po_wins_over_mo_and_mo_is_the_fallback() {
        let base = std::env::temp_dir().join(format!("kaia-i18n-mo-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let mo = |po: &str| compile_mo(&parse_po(po).unwrap(), true);
        // "xx" has a .po and a .mo for each catalog. The .po files are read, the .mo files are not.
        put(
            &base,
            "l10n/xx/koreader.po",
            "msgid \"Open\"\nmsgstr \"Ouvert (PO)\"\n".as_bytes(),
        );
        put(
            &base,
            "l10n/xx/koreader.mo",
            &mo("msgid \"Open\"\nmsgstr \"Ouvert (MO)\"\n\nmsgid \"Close\"\nmsgstr \"Fermer (MO)\"\n"),
        );
        put(
            &base,
            "l10n-kaia/xx/kaia.po",
            "msgid \"Hello\"\nmsgstr \"Salut (PO)\"\n".as_bytes(),
        );
        put(
            &base,
            "l10n-kaia/xx/kaia.mo",
            &mo("msgid \"Hello\"\nmsgstr \"Salut (MO)\"\n\nmsgid \"Scan\"\nmsgstr \"Scan (MO)\"\n"),
        );
        // "yy" has only a .mo file, which is the fallback.
        put(
            &base,
            "l10n/yy/koreader.mo",
            &mo("msgid \"Open\"\nmsgstr \"Ouvrir (MO)\"\n"),
        );
        let xx = Catalog::load(&base, "xx").unwrap();
        assert_eq!(xx.translate("Open"), "Ouvert (PO)");
        assert_eq!(xx.translate("Close"), "Close");
        assert_eq!(xx.translate("Hello"), "Salut (PO)");
        assert_eq!(xx.translate("Scan"), "Scan");
        let yy = Catalog::load(&base, "yy").unwrap();
        assert_eq!(yy.translate("Open"), "Ouvrir (MO)");
        assert!(has_catalog(&base, "xx") && has_catalog(&base, "yy"));
        assert!(!has_catalog(&base, "zz"));
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn every_koreader_mo_in_the_submodule_loads() {
        let Ok(dirs) = fs::read_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../l10n")) else {
            eprintln!("skipped: the l10n submodule is not checked out");
            return;
        };
        let mut loaded = 0;
        for dir in dirs.flatten() {
            let mo = dir.path().join("koreader.mo");
            if let Ok(bytes) = fs::read(&mo) {
                let catalog = Catalog::from_mo(&bytes).unwrap_or_else(|e| panic!("{}: {e}", mo.display()));
                assert!(catalog.plural.count >= 1, "{}", mo.display());
                loaded += 1;
            }
        }
        assert!(loaded >= 50, "{loaded} compiled catalogs");
    }

    #[test]
    fn bad_mo_files_are_refused() {
        assert!(parse_mo(b"").unwrap_err().contains("too short"));
        assert!(parse_mo(b"hello world").unwrap_err().contains("bad magic"));
        let mut cut = compile_mo(&parse_po(PO).unwrap(), true);
        cut.truncate(40);
        assert!(parse_mo(&cut).is_err());
    }
}
