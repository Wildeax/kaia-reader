//! Gettext catalog loading and lookup for the Kaia UI.
use std::collections::HashMap;
use std::fs;
use std::path::Path;

#[derive(Clone, Debug, Default)]
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
    let mut hdr = String::new();
    if let Ok(es) = parse_po(po) {
        if let Some(e) = es.iter().find(|e| e.id.is_empty()) {
            hdr = e.forms.first().cloned().unwrap_or_default()
        }
    }
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
impl Catalog {
    /// Load Kaia and KOReader PO files beneath `dir`; regional tags fall back to their base language.
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
            for source in [
                root.join("l10n-kaia").join(&tag).join("kaia.po"),
                root.join("l10n").join(&tag).join("koreader.po"),
            ] {
                if !source.exists() {
                    continue;
                }
                let raw = fs::read_to_string(&source).map_err(|e| format!("{}: {e}", source.display()))?;
                if first_header.is_none() {
                    first_header = Some(raw.clone())
                }
                for e in parse_po(&raw)? {
                    if !e.id.is_empty() {
                        cat.entries.entry((e.context.clone(), e.id.clone())).or_insert(e);
                    }
                }
            }
        }
        if let Some(h) = first_header {
            cat.plural = parse_rule(&h)
        }
        Ok(cat)
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
