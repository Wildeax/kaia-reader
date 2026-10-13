//! Reader and writer for the Lua-table data files KOReader writes.
//!
//! KOReader stores settings, book sidecars (`metadata.*.lua`) and some
//! append-mostly data as Lua chunks of the form `return { ... }`. The layout
//! comes from `frontend/dump.lua`. This crate parses that subset without
//! running any Lua code and writes the same layout back, so KOReader can read
//! the files too.
//!
//! The parser accepts literal values only: strings, numbers, booleans, `nil`
//! and table constructors. Names, calls and operators are errors, except
//! unary minus and division by the literal `0`, which is how infinities and
//! NaN are spelled.
//!
//! Lua strings can contain arbitrary bytes. Since [`Value::Str`] stores UTF-8,
//! the parser replaces invalid UTF-8 sequences with U+FFFD. This keeps the
//! containing file readable, but writing the value back cannot recover those
//! original bytes.

mod lexer;
mod parser;
mod writer;

pub use kaia_events::{Key, Table, Value};
pub use parser::{parse, ParseError};
pub use writer::{to_string, write_file, WriteError};

/// Deepest table nesting the parser accepts. Deeper input is an error, so a
/// hostile file cannot overflow the stack.
///
/// LuaJIT, which KOReader uses to load these files, stops at 197 nested
/// tables in the layout `dump.lua` writes ("chunk has too many syntax
/// levels"). The limit stays below that, so every file this crate writes
/// also loads in KOReader.
pub const MAX_DEPTH: usize = 190;

/// Largest input the parser accepts, in bytes.
pub const MAX_INPUT_BYTES: usize = 64 * 1024 * 1024;
