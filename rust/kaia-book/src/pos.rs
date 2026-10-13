//! Engine positions for text: an xpointer for reflowable books, or a point on
//! a fixed page.
//!
//! The point form follows KOReader's PDF positions. KOReader stores the page
//! as a 1-based number, `x` and `y` at zoom 1, plus the displayed zoom
//! factor and `rotation`. A point maps to the character whose box contains it, or
//! the nearest one.

use kaia_doc::Location;
use kaia_events::{Table, Value};

/// A text position in a document.
#[derive(Debug, Clone, PartialEq)]
pub enum Pos {
    /// A CREngine xpointer, stored verbatim.
    Xpointer(String),
    /// A point on a fixed page. `page` is 0-based.
    Point { page: u32, x: f32, y: f32 },
}

impl Pos {
    /// The location of the position. Xpointers keep their text, and points
    /// map to their page.
    pub fn location(&self) -> Location {
        match self {
            Pos::Xpointer(xp) => Location::Engine(xp.clone()),
            Pos::Point { page, .. } => Location::Page(*page),
        }
    }

    /// The KOReader form of the position: a string or a table.
    pub(crate) fn to_value(&self) -> Value {
        match self {
            Pos::Xpointer(xp) => Value::Str(xp.clone()),
            Pos::Point { page, x, y } => {
                let mut t = Table::new();
                t.insert("page".into(), Value::from(*page as i64 + 1));
                t.insert("x".into(), Value::from(*x as f64));
                t.insert("y".into(), Value::from(*y as f64));
                t.insert("zoom".into(), Value::from(1i64));
                t.insert("rotation".into(), Value::from(0i64));
                Value::Table(t)
            }
        }
    }

    /// Reads the KOReader form. Returns None for a value that is neither a
    /// string nor a table with a page. KOReader already stores the coordinates
    /// at zoom 1, regardless of the table's `zoom` value.
    pub(crate) fn from_value(v: &Value) -> Option<Pos> {
        match v {
            Value::Str(xp) if !xp.is_empty() => Some(Pos::Xpointer(xp.clone())),
            Value::Table(_) => {
                let page = v.get("page").as_i64()?;
                if page < 1 {
                    return None;
                }
                let x = v.get("x").as_f64().unwrap_or(0.0);
                let y = v.get("y").as_f64().unwrap_or(0.0);
                Some(Pos::Point {
                    page: (page - 1) as u32,
                    x: x as f32,
                    y: y as f32,
                })
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn point_round_trips_with_one_based_page() {
        let p = Pos::Point {
            page: 4,
            x: 10.5,
            y: 20.0,
        };
        let v = p.to_value();
        assert_eq!(v.get("page").as_i64(), Some(5));
        assert_eq!(Pos::from_value(&v), Some(p));
    }

    #[test]
    fn koreader_point_is_already_at_zoom_one() {
        let mut t = Table::new();
        t.insert("page".into(), Value::from(2i64));
        t.insert("x".into(), Value::from(300.0));
        t.insert("y".into(), Value::from(600.0));
        t.insert("zoom".into(), Value::from(2.0));
        let pos = Pos::from_value(&Value::Table(t)).expect("point");
        assert_eq!(
            pos,
            Pos::Point {
                page: 1,
                x: 300.0,
                y: 600.0
            }
        );
    }

    #[test]
    fn xpointer_string_is_kept_verbatim() {
        let v = Value::from("/body/DocFragment[2]/body/div/p[1].0");
        assert_eq!(
            Pos::from_value(&v),
            Some(Pos::Xpointer("/body/DocFragment[2]/body/div/p[1].0".to_string()))
        );
        assert_eq!(Pos::from_value(&Value::Bool(true)), None);
    }
}
