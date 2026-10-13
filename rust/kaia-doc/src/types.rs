//! Backend-neutral data types. Coordinates are page points (1/72 inch) with the
//! origin at the top left, unless a type says otherwise.

/// A point in page space.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PointF {
    pub x: f32,
    pub y: f32,
}

/// An axis-aligned box in page space: `x0 <= x1` and `y0 <= y1`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct RectF {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
}

impl RectF {
    pub const fn new(x0: f32, y0: f32, x1: f32, y1: f32) -> Self {
        Self { x0, y0, x1, y1 }
    }

    pub fn width(&self) -> f32 {
        self.x1 - self.x0
    }

    pub fn height(&self) -> f32 {
        self.y1 - self.y0
    }

    /// True when `other` lies inside `self`, edges included.
    pub fn contains_rect(&self, other: &RectF) -> bool {
        other.x0 >= self.x0 && other.y0 >= self.y0 && other.x1 <= self.x1 && other.y1 <= self.y1
    }
}

/// A page size in points.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PageSize {
    pub width: f32,
    pub height: f32,
}

/// A text search hit. MuPDF's order is upper left, upper right, lower left,
/// lower right. The quad can be rotated, so use `bounds` for a box.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Quad {
    pub ul: PointF,
    pub ur: PointF,
    pub ll: PointF,
    pub lr: PointF,
}

impl Quad {
    /// The smallest box that contains the four corners.
    pub fn bounds(&self) -> RectF {
        let xs = [self.ul.x, self.ur.x, self.ll.x, self.lr.x];
        let ys = [self.ul.y, self.ur.y, self.ll.y, self.lr.y];
        RectF::new(
            xs.iter().copied().fold(f32::INFINITY, f32::min),
            ys.iter().copied().fold(f32::INFINITY, f32::min),
            xs.iter().copied().fold(f32::NEG_INFINITY, f32::max),
            ys.iter().copied().fold(f32::NEG_INFINITY, f32::max),
        )
    }
}

/// One table of contents entry. `level` starts at 1.
#[derive(Debug, Clone, PartialEq)]
pub struct TocEntry {
    /// CREngine position, preserved across reflow. None for other backends.
    pub xpointer: Option<String>,
    pub level: u32,
    pub title: String,
    /// Target page, or None when the entry does not point into the document.
    pub page: Option<u32>,
}

/// Document metadata. Missing values are None.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Metadata {
    pub title: Option<String>,
    pub author: Option<String>,
}

/// One character of page text with its box.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextChar {
    pub ch: char,
    pub bbox: RectF,
}

/// One line of page text. `first_char` and `char_count` index into
/// [`PageText::chars`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextLine {
    pub bbox: RectF,
    pub first_char: usize,
    pub char_count: usize,
}

/// Text of one page. `text` holds the characters in reading order with a
/// newline after each line. Newlines have no entry in `chars`, so `chars[i]`
/// is the i-th character that is not a newline.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PageText {
    pub text: String,
    pub lines: Vec<TextLine>,
    pub chars: Vec<TextChar>,
}

/// Where a link points.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkTarget {
    /// A page of this document.
    Page(u32),
    /// Anything else: an external URL, or an internal target that did not resolve.
    Uri(String),
}

/// A clickable area on a page.
#[derive(Debug, Clone, PartialEq)]
pub struct Link {
    pub bbox: RectF,
    pub target: LinkTarget,
}

/// Page margins in points.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Margins {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

/// Reflowable layout settings.
///
/// `width` and `height` are the page size in points. `em` is the font size in
/// points. The optional fields are written as user CSS when they are set. None
/// leaves the book's own styling in place. `css_extra` is appended last.
#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    pub width: f32,
    pub height: f32,
    pub em: f32,
    pub margins: Option<Margins>,
    /// Unitless line height, for example 1.4.
    pub line_height: Option<f32>,
    pub font_family: Option<String>,
    pub css_extra: String,
}

impl Layout {
    /// A layout with the given page size and font size and no overrides.
    pub fn new(width: f32, height: f32, em: f32) -> Self {
        Self {
            width,
            height,
            em,
            margins: None,
            line_height: None,
            font_family: None,
            css_extra: String::new(),
        }
    }
}

/// A place in a document. `Page` is a page number. `Engine` holds an engine
/// position that only its own backend understands. For MuPDF it is an internal
/// URI such as `#chapter3`. CREngine will use xpointers in phase R5.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Location {
    Page(u32),
    Engine(String),
}
