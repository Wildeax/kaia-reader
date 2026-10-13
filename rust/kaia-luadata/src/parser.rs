//! Parser for `return <expr>` data files. It builds values directly and never
//! evaluates code.

use std::fmt;

use kaia_events::{Key, Table, Value};

use crate::lexer::{fail, Failure, Lexer, Tok, Token};
use crate::{MAX_DEPTH, MAX_INPUT_BYTES};

/// A syntax error with a 1-based line and column (counted in characters).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub line: usize,
    pub column: usize,
    pub message: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}, column {}: {}", self.line, self.column, self.message)
    }
}

impl std::error::Error for ParseError {}

/// Parses a data file: an optional comment block, then `return <expr>`.
///
/// Accepted expressions are table constructors, strings (quoted or long
/// brackets), numbers (decimal, hexadecimal, `-` prefixed, and `1/0`, `-1/0`,
/// `0/0` for infinities and NaN), `true`, `false` and `nil`. Anything else is
/// an error.
pub fn parse(src: &str) -> Result<Value, ParseError> {
    if src.len() > MAX_INPUT_BYTES {
        return Err(ParseError {
            line: 1,
            column: 1,
            message: format!("input is larger than {MAX_INPUT_BYTES} bytes"),
        });
    }
    Parser::new(src)
        .and_then(|mut p| p.file())
        .map_err(|f| locate(src, f))
}

fn locate(src: &str, f: Failure) -> ParseError {
    let mut pos = f.pos.min(src.len());
    while !src.is_char_boundary(pos) {
        pos -= 1;
    }
    let before = &src[..pos];
    let line = before.matches('\n').count() + 1;
    let line_start = before.rfind('\n').map_or(0, |i| i + 1);
    let column = src[line_start..pos].chars().count() + 1;
    ParseError {
        line,
        column,
        message: f.msg,
    }
}

const KEYWORDS: &[&str] = &[
    "and", "break", "do", "else", "elseif", "end", "false", "for", "function", "goto", "if", "in", "local",
    "nil", "not", "or", "repeat", "return", "then", "true", "until", "while",
];

fn is_keyword(name: &str) -> bool {
    KEYWORDS.contains(&name)
}

fn describe(tok: &Tok) -> String {
    match tok {
        Tok::Name(n) => format!("'{n}'"),
        Tok::Str(_) => "a string".to_owned(),
        Tok::Num(_) => "a number".to_owned(),
        Tok::Sym(c) => format!("'{}'", *c as char),
        Tok::Eof => "end of input".to_owned(),
    }
}

fn store(table: &mut Table, key: Key, value: Value) {
    // Lua stores nothing for a nil value, and a later nil removes an earlier entry.
    if value.is_nil() {
        table.remove(&key);
    } else {
        table.insert(key, value);
    }
}

fn to_key(value: Value, pos: usize) -> Result<Key, Failure> {
    match value {
        Value::Str(s) => Ok(Key::Str(s)),
        Value::Bool(b) => Ok(Key::Bool(b)),
        Value::Number(n) => match Key::num(n) {
            Some(key) => Ok(key),
            None => fail(pos, "NaN cannot be a table key"),
        },
        Value::Nil => fail(pos, "nil cannot be a table key"),
        Value::Table(_) => fail(pos, "a table cannot be a table key in data files"),
    }
}

struct Parser<'a> {
    lex: Lexer<'a>,
    cur: Token,
}

impl<'a> Parser<'a> {
    fn new(src: &'a str) -> Result<Self, Failure> {
        let mut lex = Lexer::new(src);
        let cur = lex.next_token()?;
        Ok(Parser { lex, cur })
    }

    fn advance(&mut self) -> Result<(), Failure> {
        self.cur = self.lex.next_token()?;
        Ok(())
    }

    fn is_sym(&self, c: u8) -> bool {
        self.cur.tok == Tok::Sym(c)
    }

    fn expect_sym(&mut self, c: u8, context: &str) -> Result<(), Failure> {
        if self.is_sym(c) {
            self.advance()
        } else {
            fail(
                self.cur.pos,
                format!(
                    "expected '{}' {context}, found {}",
                    c as char,
                    describe(&self.cur.tok)
                ),
            )
        }
    }

    fn file(&mut self) -> Result<Value, Failure> {
        if self.cur.tok != Tok::Name("return".to_owned()) {
            return fail(
                self.cur.pos,
                format!("expected 'return', found {}", describe(&self.cur.tok)),
            );
        }
        self.advance()?;
        let value = self.expr(0)?;
        if self.is_sym(b';') {
            self.advance()?;
        }
        if self.cur.tok != Tok::Eof {
            return fail(
                self.cur.pos,
                format!("unexpected {} after the return value", describe(&self.cur.tok)),
            );
        }
        Ok(value)
    }

    /// Parses one expression. `depth` is the number of tables that enclose it.
    fn expr(&mut self, depth: usize) -> Result<Value, Failure> {
        let pos = self.cur.pos;
        match self.cur.tok.clone() {
            Tok::Str(s) => {
                self.advance()?;
                Ok(Value::Str(s))
            }
            Tok::Num(n) => {
                self.advance()?;
                self.number_tail(n)
            }
            Tok::Sym(b'-') => {
                self.advance()?;
                match self.cur.tok {
                    Tok::Num(n) => {
                        self.advance()?;
                        self.number_tail(-n)
                    }
                    // KOReader's dump writes `-inf`. LuaJIT reads `inf` as an
                    // undefined global, so KOReader sees nil. Import nil too.
                    Tok::Name(ref n) if n == "inf" || n == "nan" => {
                        self.advance()?;
                        Ok(Value::Nil)
                    }
                    _ => fail(self.cur.pos, "expected a number after '-'"),
                }
            }
            Tok::Sym(b'{') => self.table(depth + 1),
            Tok::Name(name) => match name.as_str() {
                "true" => {
                    self.advance()?;
                    Ok(Value::Bool(true))
                }
                "false" => {
                    self.advance()?;
                    Ok(Value::Bool(false))
                }
                "nil" => {
                    self.advance()?;
                    Ok(Value::Nil)
                }
                // KOReader's dump writes `inf` and `nan` for those numbers.
                // LuaJIT reads them as undefined globals, which are nil, so
                // KOReader itself sees nil. Import them as nil too instead of
                // rejecting the whole file.
                "inf" | "nan" => {
                    self.advance()?;
                    Ok(Value::Nil)
                }
                _ if is_keyword(&name) => fail(pos, format!("unexpected keyword '{name}'")),
                _ => {
                    self.advance()?;
                    self.name_error(&name, pos)
                }
            },
            other => fail(pos, format!("expected a value, found {}", describe(&other))),
        }
    }

    /// Handles `/ 0` after a number, which spells infinity or NaN.
    fn number_tail(&mut self, numerator: f64) -> Result<Value, Failure> {
        if !self.is_sym(b'/') {
            return Ok(Value::Number(numerator));
        }
        let slash = self.cur.pos;
        self.advance()?;
        let divisor = match self.cur.tok {
            Tok::Num(d) => Some(d),
            _ => None,
        };
        match divisor {
            Some(d) if d == 0.0 => {
                self.advance()?;
                Ok(Value::Number(numerator / d))
            }
            _ => fail(
                slash,
                "division is only allowed by the literal 0 (1/0, -1/0 and 0/0 for infinity and NaN)",
            ),
        }
    }

    /// Reports a name used as a value. Called after the name is consumed, so
    /// `self.cur` is the token that follows it.
    fn name_error<T>(&self, name: &str, pos: usize) -> Result<T, Failure> {
        if matches!(self.cur.tok, Tok::Str(_) | Tok::Sym(b'(') | Tok::Sym(b'{')) {
            fail(pos, format!("function calls are not allowed, found '{name}('"))
        } else {
            fail(
                pos,
                format!("names are not allowed in data files, found '{name}'"),
            )
        }
    }

    /// Parses a table constructor. `depth` is this table's own nesting level.
    fn table(&mut self, depth: usize) -> Result<Value, Failure> {
        if depth > MAX_DEPTH {
            return fail(
                self.cur.pos,
                format!("tables are nested deeper than {MAX_DEPTH} levels"),
            );
        }
        self.advance()?;
        let mut table = Table::new();
        let mut positional: Vec<Value> = Vec::new();
        loop {
            if self.is_sym(b'}') {
                self.advance()?;
                break;
            }
            let field_pos = self.cur.pos;
            match self.cur.tok.clone() {
                Tok::Sym(b'[') => {
                    self.advance()?;
                    let key_pos = self.cur.pos;
                    let key = self.expr(depth)?;
                    self.expect_sym(b']', "to close the key")?;
                    self.expect_sym(b'=', "after the key")?;
                    let value = self.expr(depth)?;
                    store(&mut table, to_key(key, key_pos)?, value);
                }
                Tok::Name(name) if !is_keyword(&name) => {
                    self.advance()?;
                    if !self.is_sym(b'=') {
                        return self.name_error(&name, field_pos);
                    }
                    self.advance()?;
                    let value = self.expr(depth)?;
                    store(&mut table, Key::Str(name), value);
                }
                _ => positional.push(self.expr(depth)?),
            }
            if self.is_sym(b',') || self.is_sym(b';') {
                self.advance()?;
            } else if !self.is_sym(b'}') {
                return fail(
                    self.cur.pos,
                    format!("expected ',' or '}}' in table, found {}", describe(&self.cur.tok)),
                );
            }
        }
        // Positional entries get keys 1, 2, ... and win over explicit keys, as in Lua.
        for (i, value) in positional.into_iter().enumerate() {
            store(&mut table, Key::Int(i as i64 + 1), value);
        }
        Ok(Value::Table(table))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_small_table() {
        let v = parse("-- x\nreturn { [\"a\"] = 1, b = \"two\", 3; }").unwrap();
        assert_eq!(v.get("a"), &Value::Number(1.0));
        assert_eq!(v.get("b"), &Value::from("two"));
        assert_eq!(v.get(1i64), &Value::Number(3.0));
    }

    #[test]
    fn error_positions_count_lines_and_columns() {
        let err = parse("return {\n  a = ,\n}").unwrap_err();
        assert_eq!((err.line, err.column), (2, 7));
    }
}
