//! Highlights, notes and bookmarks, with KOReader's field names.
//!
//! A bookmark is an annotation with no `drawer`, as in KOReader's
//! `annotations` table. Positions are never stored as screen coordinates.
//! They are engine positions, and boxes are computed when a page is drawn.

use kaia_doc::Location;
use kaia_events::{Table, Value};

use crate::pos::Pos;

/// The id of an annotation within one book. Assigned by the session.
pub type AnnotationId = u64;

/// Highlight styles, as KOReader names them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Drawer {
    Lighten,
    Underscore,
    Strikeout,
    Invert,
}

impl Drawer {
    /// The KOReader name, such as `lighten`.
    pub fn as_str(self) -> &'static str {
        match self {
            Drawer::Lighten => "lighten",
            Drawer::Underscore => "underscore",
            Drawer::Strikeout => "strikeout",
            Drawer::Invert => "invert",
        }
    }

    /// Parses a KOReader drawer name.
    pub fn parse(name: &str) -> Option<Drawer> {
        match name {
            "lighten" => Some(Drawer::Lighten),
            "underscore" => Some(Drawer::Underscore),
            "strikeout" => Some(Drawer::Strikeout),
            "invert" => Some(Drawer::Invert),
            _ => None,
        }
    }
}

/// The highlight colors of KOReader, in its menu order.
pub const HIGHLIGHT_COLORS: [&str; 9] = [
    "red", "orange", "yellow", "green", "olive", "cyan", "blue", "purple", "gray",
];

/// The drawer styles of KOReader, in its menu order.
pub const HIGHLIGHT_DRAWERS: [Drawer; 4] = [
    Drawer::Lighten,
    Drawer::Underscore,
    Drawer::Strikeout,
    Drawer::Invert,
];

/// The RGB value of a highlight color, as KOReader's `Blitbuffer.HIGHLIGHT_COLORS`
/// gives it. `gray` is KOReader's `Blitbuffer.gray(0.2)`, which is level 204. Returns
/// None for a name KOReader does not know.
///
/// A lighten highlight multiplies the page by this value, so `gray` darkens the
/// page by 20 percent, as KOReader's grayscale highlight does. On a grayscale panel
/// the luminance of the value is what shows.
pub fn highlight_rgb(name: &str) -> Option<[u8; 3]> {
    Some(match name {
        "red" => [0xFF, 0x33, 0x00],
        "orange" => [0xFF, 0x88, 0x00],
        "yellow" => [0xFF, 0xFF, 0x33],
        "green" => [0x00, 0xAA, 0x66],
        "olive" => [0x88, 0xFF, 0x77],
        "cyan" => [0x00, 0xFF, 0xEE],
        "blue" => [0x00, 0x66, 0xFF],
        "purple" => [0xEE, 0x00, 0xFF],
        "gray" => [204, 204, 204],
        _ => return None,
    })
}

/// A highlight, note or bookmark.
#[derive(Debug, Clone, PartialEq)]
pub struct Annotation {
    pub id: AnnotationId,
    /// Creation time, `YYYY-MM-DD HH:MM:SS`.
    pub datetime: String,
    /// Time of the last edit, same format.
    pub datetime_updated: Option<String>,
    /// Highlight style. None for a bookmark.
    pub drawer: Option<String>,
    /// Highlight color name, such as `yellow`.
    pub color: Option<String>,
    /// The highlighted text, as selected.
    pub text: Option<String>,
    /// Whether the user edited the selected text.
    pub text_edited: Option<bool>,
    /// The user's note.
    pub note: Option<String>,
    pub note_format: Option<String>,
    /// Chapter title at the position, when the book has a table of contents.
    pub chapter: Option<String>,
    /// Page number, 1-based, as of the last computation. Not stored in
    /// positions, so it can differ after a relayout until the next listing.
    pub pageno: Option<u32>,
    pub pageref: Option<Value>,
    pub pboxes: Option<Value>,
    pub ext: Option<Value>,
    /// The page or xpointer of the annotation. For a highlight, the start.
    pub page: Location,
    /// Start of a highlight.
    pub pos0: Option<Pos>,
    /// End of a highlight.
    pub pos1: Option<Pos>,
    /// True when the position no longer resolves in the document. The
    /// annotation is kept, and the Notes surface lists it for repair.
    pub orphaned: bool,
}

impl Annotation {
    /// True for a bookmark: an annotation with no highlight style.
    pub fn is_bookmark(&self) -> bool {
        self.drawer.is_none()
    }

    /// The KOReader table form. `page` holds an xpointer string or a 1-based
    /// page number, and the positions hold KOReader's shapes.
    pub fn to_value(&self) -> Value {
        let mut t = Table::new();
        let mut put = |k: &str, v: Value| {
            if !v.is_nil() {
                t.insert(k.into(), v);
            }
        };
        put("kaia_id", Value::from(self.id as i64));
        put("datetime", opt_str(Some(&self.datetime)));
        put("datetime_updated", opt_str(self.datetime_updated.as_deref()));
        put("drawer", opt_str(self.drawer.as_deref()));
        put("color", opt_str(self.color.as_deref()));
        put("text", opt_str(self.text.as_deref()));
        put("text_edited", self.text_edited.map_or(Value::Nil, Value::Bool));
        put("note", opt_str(self.note.as_deref()));
        put("note_format", opt_str(self.note_format.as_deref()));
        put("chapter", opt_str(self.chapter.as_deref()));
        put(
            "pageno",
            self.pageno.map_or(Value::Nil, |n| Value::from(n as i64)),
        );
        put(
            "page",
            match &self.page {
                Location::Engine(xp) => Value::Str(xp.clone()),
                Location::Page(p) => Value::from(*p as i64 + 1),
            },
        );
        put("pos0", self.pos0.as_ref().map_or(Value::Nil, Pos::to_value));
        put("pos1", self.pos1.as_ref().map_or(Value::Nil, Pos::to_value));
        put("pageref", self.pageref.clone().unwrap_or(Value::Nil));
        put("pboxes", self.pboxes.clone().unwrap_or(Value::Nil));
        put("ext", self.ext.clone().unwrap_or(Value::Nil));
        if self.orphaned {
            put("orphaned", Value::Bool(true));
        }
        Value::Table(t)
    }

    /// Reads the table form. The id is `kaia_id`, or 0 when the table has none
    /// (a KOReader sidecar). The caller assigns ids to those.
    pub(crate) fn from_value(v: &Value) -> Option<Annotation> {
        let page = match v.get("page") {
            Value::Str(xp) if !xp.is_empty() => Location::Engine(xp.clone()),
            Value::Number(n) if *n >= 1.0 && n.fract() == 0.0 => Location::Page(*n as u32 - 1),
            _ => return None,
        };
        let text_of = |key: &str| v.get(key).as_str().map(str::to_string);
        let pos_of = |key: &str| match v.get(key) {
            Value::Nil => Some(None),
            other => Pos::from_value(other).map(Some),
        };
        let pos0 = pos_of("pos0")?;
        let pos1 = pos_of("pos1")?;
        let datetime = text_of("datetime").unwrap_or_default();
        let id = v
            .get("kaia_id")
            .as_i64()
            .filter(|n| *n > 0)
            .map_or(0, |n| n as u64);
        Some(Annotation {
            id,
            datetime,
            datetime_updated: text_of("datetime_updated"),
            drawer: text_of("drawer"),
            color: text_of("color"),
            text: text_of("text"),
            text_edited: v.get("text_edited").as_bool(),
            note: text_of("note"),
            note_format: text_of("note_format"),
            chapter: text_of("chapter"),
            pageno: v.get("pageno").as_i64().filter(|n| *n > 0).map(|n| n as u32),
            pageref: (!v.get("pageref").is_nil()).then(|| v.get("pageref").clone()),
            pboxes: (!v.get("pboxes").is_nil()).then(|| v.get("pboxes").clone()),
            ext: (!v.get("ext").is_nil()).then(|| v.get("ext").clone()),
            page,
            pos0,
            pos1,
            orphaned: v.get("orphaned").as_bool().unwrap_or(false),
        })
    }
}

fn opt_str(s: Option<&str>) -> Value {
    s.map_or(Value::Nil, |s| Value::Str(s.to_string()))
}

/// An annotation with its boxes on one page, in screen pixels of the viewport
/// the boxes were computed for.
#[derive(Debug, Clone, PartialEq)]
pub struct PageAnnotation {
    pub annotation: Annotation,
    pub rects: Vec<kaia_gfx::Rect>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Annotation {
        Annotation {
            id: 7,
            datetime: "2026-10-13 09:00:00".into(),
            datetime_updated: None,
            drawer: Some("lighten".into()),
            color: Some("yellow".into()),
            text: Some("Call me Ishmael.".into()),
            text_edited: None,
            note: None,
            note_format: None,
            chapter: Some("Loomings".into()),
            pageno: Some(3),
            pageref: None,
            pboxes: None,
            ext: None,
            page: Location::Engine("/body/DocFragment[2]/body/div/p[2].0".into()),
            pos0: Some(Pos::Xpointer("/body/DocFragment[2]/body/div/p[2].0".into())),
            pos1: Some(Pos::Xpointer("/body/DocFragment[2]/body/div/p[2].40".into())),
            orphaned: false,
        }
    }

    #[test]
    fn value_round_trip_keeps_koreader_fields() {
        let mut a = sample();
        a.text_edited = Some(true);
        a.note_format = Some("md".into());
        a.pageref = Some(Value::from("12"));
        a.pboxes = Some(Value::array([Value::from(4i64)]));
        a.ext = Some(Value::Bool(true));
        let v = a.to_value();
        assert_eq!(v.get("drawer").as_str(), Some("lighten"));
        assert_eq!(v.get("text").as_str(), Some("Call me Ishmael."));
        assert!(v.get("note").is_nil());
        let back = Annotation::from_value(&v).expect("annotation");
        assert_eq!(back.page, a.page);
        assert_eq!(back.pos1, a.pos1);
        assert_eq!(back.chapter, a.chapter);
        assert_eq!(back.text_edited, a.text_edited);
        assert_eq!(back.note_format, a.note_format);
        assert_eq!(back.pageref, a.pageref);
        assert_eq!(back.pboxes, a.pboxes);
        assert_eq!(back.ext, a.ext);
        assert!(!back.is_bookmark());
    }

    #[test]
    fn fixed_page_bookmark_uses_one_based_number() {
        let mut a = sample();
        a.drawer = None;
        a.page = Location::Page(4);
        a.pos0 = None;
        a.pos1 = None;
        let v = a.to_value();
        assert_eq!(v.get("page").as_i64(), Some(5));
        let back = Annotation::from_value(&v).expect("bookmark");
        assert!(back.is_bookmark());
        assert_eq!(back.page, Location::Page(4));
    }

    #[test]
    fn drawer_names() {
        assert_eq!(Drawer::parse("strikeout"), Some(Drawer::Strikeout));
        assert_eq!(Drawer::Invert.as_str(), "invert");
        assert_eq!(Drawer::parse("wavy"), None);
    }

    #[test]
    fn every_listed_color_has_a_value_and_gray_is_level_204() {
        for name in HIGHLIGHT_COLORS {
            assert!(highlight_rgb(name).is_some(), "{name} has no RGB value");
        }
        assert_eq!(highlight_rgb("gray"), Some([204, 204, 204]));
        assert_eq!(highlight_rgb("yellow"), Some([0xFF, 0xFF, 0x33]));
        assert_eq!(highlight_rgb("teal"), None, "KOReader has no teal");
        assert_eq!(highlight_rgb("Yellow"), None, "names are stored in lower case");
    }

    #[test]
    fn drawers_are_in_koreader_order() {
        let names: Vec<&str> = HIGHLIGHT_DRAWERS.iter().map(|d| d.as_str()).collect();
        assert_eq!(names, ["lighten", "underscore", "strikeout", "invert"]);
        for drawer in HIGHLIGHT_DRAWERS {
            assert_eq!(Drawer::parse(drawer.as_str()), Some(drawer));
        }
    }
}
