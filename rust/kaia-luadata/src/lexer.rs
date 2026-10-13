//! Tokens for the Lua subset that KOReader data files use.

/// A syntax error at a byte offset into the source.
#[derive(Debug)]
pub(crate) struct Failure {
    pub(crate) pos: usize,
    pub(crate) msg: String,
}

pub(crate) fn fail<T>(pos: usize, msg: impl Into<String>) -> Result<T, Failure> {
    Err(Failure { pos, msg: msg.into() })
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Tok {
    Name(String),
    Str(String),
    Num(f64),
    /// A one-byte symbol: one of `{ } [ ] = , ; - / . ( ) :`.
    Sym(u8),
    Eof,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Token {
    pub(crate) tok: Tok,
    pub(crate) pos: usize,
}

pub(crate) struct Lexer<'a> {
    src: &'a str,
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Lexer<'a> {
    pub(crate) fn new(src: &'a str) -> Self {
        Lexer {
            src,
            bytes: src.as_bytes(),
            pos: 0,
        }
    }

    pub(crate) fn next_token(&mut self) -> Result<Token, Failure> {
        self.skip_space_and_comments()?;
        let start = self.pos;
        let Some(&c) = self.bytes.get(start) else {
            return Ok(Token {
                tok: Tok::Eof,
                pos: start,
            });
        };
        let next = self.bytes.get(start + 1).copied();
        let tok = match c {
            b'[' if long_bracket_level(self.bytes, start).is_some() => {
                let (s, end) = read_long_bracket(self.src, start, "string")?;
                self.pos = end;
                Tok::Str(s)
            }
            b'"' | b'\'' => {
                let (s, end) = read_quoted(self.src, start)?;
                self.pos = end;
                Tok::Str(s)
            }
            b'0'..=b'9' => self.number(start)?,
            b'.' if next.is_some_and(|d| d.is_ascii_digit()) => self.number(start)?,
            b'_' | b'a'..=b'z' | b'A'..=b'Z' => {
                let mut end = start;
                while self.bytes.get(end).is_some_and(|&b| is_name_byte(b)) {
                    end += 1;
                }
                self.pos = end;
                Tok::Name(self.src[start..end].to_owned())
            }
            b'{' | b'}' | b'[' | b']' | b'=' | b',' | b';' | b'-' | b'/' | b'.' | b'(' | b')' | b':' => {
                self.pos += 1;
                Tok::Sym(c)
            }
            _ => {
                let ch = self.src[start..].chars().next().unwrap_or('?');
                return fail(start, format!("unexpected character '{ch}'"));
            }
        };
        Ok(Token { tok, pos: start })
    }

    fn number(&mut self, start: usize) -> Result<Tok, Failure> {
        let (n, end) = read_number(self.src, start)?;
        self.pos = end;
        Ok(Tok::Num(n))
    }

    /// Skips spaces, newlines, `--` line comments and `--[[ ]]` block comments.
    fn skip_space_and_comments(&mut self) -> Result<(), Failure> {
        loop {
            match self.bytes.get(self.pos) {
                Some(&c) if is_space(c) => self.pos += 1,
                Some(b'-') if self.bytes.get(self.pos + 1) == Some(&b'-') => {
                    let comment_start = self.pos;
                    self.pos += 2;
                    if long_bracket_level(self.bytes, self.pos).is_some() {
                        let open = self.pos;
                        let (_, end) = read_long_bracket(self.src, open, "comment").map_err(|mut f| {
                            f.pos = comment_start;
                            f
                        })?;
                        self.pos = end;
                    } else {
                        while self.bytes.get(self.pos).is_some_and(|&c| c != b'\n') {
                            self.pos += 1;
                        }
                    }
                }
                _ => return Ok(()),
            }
        }
    }
}

fn is_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\r' | b'\n' | 0x0b | 0x0c)
}

fn is_name_byte(c: u8) -> bool {
    c == b'_' || c.is_ascii_alphanumeric()
}

fn hex_value(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

/// If a long bracket `[` `=`* `[` starts at `pos`, returns its level, the
/// number of `=` signs.
fn long_bracket_level(bytes: &[u8], pos: usize) -> Option<usize> {
    if bytes.get(pos) != Some(&b'[') {
        return None;
    }
    let mut p = pos + 1;
    while bytes.get(p) == Some(&b'=') {
        p += 1;
    }
    (bytes.get(p) == Some(&b'[')).then_some(p - pos - 1)
}

/// Reads a long bracket whose opening `[` is at `open`. Returns the content
/// and the offset just after the closing bracket. The level must already be
/// known to be valid, see [`long_bracket_level`].
fn read_long_bracket(src: &str, open: usize, what: &str) -> Result<(String, usize), Failure> {
    let bytes = src.as_bytes();
    let level = long_bracket_level(bytes, open).unwrap_or(0);
    let mut p = open + level + 2;
    // Lua drops a newline that directly follows the opening bracket.
    match bytes.get(p) {
        Some(b'\r') => {
            p += 1;
            if bytes.get(p) == Some(&b'\n') {
                p += 1;
            }
        }
        Some(b'\n') => {
            p += 1;
            if bytes.get(p) == Some(&b'\r') {
                p += 1;
            }
        }
        _ => {}
    }
    let close = format!("]{}]", "=".repeat(level));
    match src[p..].find(&close) {
        Some(i) => Ok((src[p..p + i].to_owned(), p + i + close.len())),
        None => fail(open, format!("unfinished long {what}")),
    }
}

/// Reads a quoted string that starts at `start`. Returns the decoded text and
/// the offset just after the closing quote.
fn read_quoted(src: &str, start: usize) -> Result<(String, usize), Failure> {
    let bytes = src.as_bytes();
    let quote = bytes[start];
    let mut out = Vec::new();
    let mut p = start + 1;
    loop {
        let Some(&c) = bytes.get(p) else {
            return fail(start, "unfinished string");
        };
        if c == quote {
            p += 1;
            break;
        }
        match c {
            b'\n' | b'\r' => return fail(p, "unfinished string"),
            b'\\' => p = read_escape(bytes, p + 1, &mut out)?,
            _ => {
                out.push(c);
                p += 1;
            }
        }
    }
    // The shared Value type stores UTF-8 strings. Preserve valid text and
    // replace invalid byte sequences so one old Lua byte string cannot reject
    // the rest of a settings or sidecar file.
    Ok((String::from_utf8_lossy(&out).into_owned(), p))
}

/// Decodes one escape. `p` is the offset just after the backslash. Returns
/// the offset after the escape.
fn read_escape(bytes: &[u8], p: usize, out: &mut Vec<u8>) -> Result<usize, Failure> {
    let backslash = p - 1;
    let Some(&e) = bytes.get(p) else {
        return fail(backslash, "unfinished string");
    };
    let simple = match e {
        b'a' => Some(7),
        b'b' => Some(8),
        b'f' => Some(12),
        b'n' => Some(b'\n'),
        b'r' => Some(b'\r'),
        b't' => Some(b'\t'),
        b'v' => Some(11),
        b'\\' | b'"' | b'\'' => Some(e),
        _ => None,
    };
    if let Some(byte) = simple {
        out.push(byte);
        return Ok(p + 1);
    }
    match e {
        b'\n' | b'\r' => {
            // A backslash before a line break is a line break. CR LF and LF CR count as one.
            out.push(b'\n');
            let other = if e == b'\n' { b'\r' } else { b'\n' };
            Ok(if bytes.get(p + 1) == Some(&other) {
                p + 2
            } else {
                p + 1
            })
        }
        b'x' => match (
            bytes.get(p + 1).and_then(|&c| hex_value(c)),
            bytes.get(p + 2).and_then(|&c| hex_value(c)),
        ) {
            (Some(hi), Some(lo)) => {
                out.push(hi * 16 + lo);
                Ok(p + 3)
            }
            _ => fail(backslash, "\\x needs two hex digits"),
        },
        b'z' => {
            // Skips the whitespace that follows.
            let mut q = p + 1;
            while bytes.get(q).is_some_and(|&c| is_space(c)) {
                q += 1;
            }
            Ok(q)
        }
        b'0'..=b'9' => {
            let mut q = p;
            let mut value: u32 = 0;
            while q < p + 3 {
                match bytes.get(q) {
                    Some(&d) if d.is_ascii_digit() => {
                        value = value * 10 + u32::from(d - b'0');
                        q += 1;
                    }
                    _ => break,
                }
            }
            if value > 255 {
                return fail(backslash, "decimal escape is larger than 255");
            }
            out.push(value as u8);
            Ok(q)
        }
        // Lua 5.1 and LuaJIT keep the character after an unknown escape.
        other => {
            out.push(other);
            Ok(p + 1)
        }
    }
}

/// Reads a numeral that starts at `start`: decimal with optional fraction and
/// exponent, or hexadecimal integer.
fn read_number(src: &str, start: usize) -> Result<(f64, usize), Failure> {
    let bytes = src.as_bytes();
    let mut p = start;
    let value = if bytes[start] == b'0' && matches!(bytes.get(start + 1), Some(b'x' | b'X')) {
        p = start + 2;
        let digits_start = p;
        let mut v = 0.0_f64;
        while let Some(d) = bytes.get(p).and_then(|&c| hex_value(c)) {
            v = v * 16.0 + f64::from(d);
            p += 1;
        }
        if p == digits_start {
            return fail(start, "malformed hexadecimal number");
        }
        v
    } else {
        while bytes.get(p).is_some_and(u8::is_ascii_digit) {
            p += 1;
        }
        if bytes.get(p) == Some(&b'.') {
            p += 1;
            while bytes.get(p).is_some_and(u8::is_ascii_digit) {
                p += 1;
            }
        }
        if matches!(bytes.get(p), Some(b'e' | b'E')) {
            p += 1;
            if matches!(bytes.get(p), Some(b'+' | b'-')) {
                p += 1;
            }
            let exp_start = p;
            while bytes.get(p).is_some_and(u8::is_ascii_digit) {
                p += 1;
            }
            if p == exp_start {
                return fail(start, "malformed number: the exponent has no digits");
            }
        }
        let text = &src[start..p];
        match text.parse::<f64>() {
            Ok(v) => v,
            Err(_) => return fail(start, format!("malformed number '{text}'")),
        }
    };
    if bytes
        .get(p)
        .is_some_and(|&c| c == b'_' || c == b'.' || c.is_ascii_alphanumeric())
    {
        return fail(start, "malformed number");
    }
    Ok((value, p))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens(src: &str) -> Vec<Tok> {
        let mut lex = Lexer::new(src);
        let mut out = Vec::new();
        loop {
            let t = lex
                .next_token()
                .unwrap_or_else(|f| panic!("{}: {}", f.pos, f.msg));
            if t.tok == Tok::Eof {
                return out;
            }
            out.push(t.tok);
        }
    }

    #[test]
    fn numbers_and_names() {
        assert_eq!(
            tokens("1 .5 5. 1e3 0x1F -2 abc_1"),
            vec![
                Tok::Num(1.0),
                Tok::Num(0.5),
                Tok::Num(5.0),
                Tok::Num(1000.0),
                Tok::Num(31.0),
                Tok::Sym(b'-'),
                Tok::Num(2.0),
                Tok::Name("abc_1".into()),
            ]
        );
    }

    #[test]
    fn comments_are_skipped() {
        assert_eq!(
            tokens("-- line\n--[==[ block ]] ]==] 1 --[ not block\n2"),
            vec![Tok::Num(1.0), Tok::Num(2.0)]
        );
    }

    #[test]
    fn long_strings_drop_the_first_newline() {
        assert_eq!(tokens("[[\nab\ncd]]"), vec![Tok::Str("ab\ncd".into())]);
        assert_eq!(tokens("[==[a]]b]==]"), vec![Tok::Str("a]]b".into())]);
    }

    #[test]
    fn malformed_numbers_are_errors() {
        for bad in ["1e", "0x", "3abc", "1..2"] {
            let mut lex = Lexer::new(bad);
            assert!(lex.next_token().is_err(), "{bad} should not lex");
        }
    }
}
