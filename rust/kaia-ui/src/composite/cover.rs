//! [`CoverImage`] and [`BookTile`]: book covers for lists and grids.

use kaia_events::Value;
use kaia_gfx::image::{fit_pixmap, FitMode};
use kaia_gfx::{Pixmap, Point, Rect, Size};
use kaia_input::Gesture;

use crate::components::{Column, CrossAlign, Label, Paragraph};
use crate::controls::Reaction;
use crate::geometry::Constraints;
use crate::pod::WidgetPod;
use crate::theme::{Theme, Tone};
use crate::widget::{EventCx, Handled, LayoutCx, PaintCx, UiEvent, Widget};

/// A book cover in a 2 by 3 box with a hairline border. Without a cover
/// the box shows the title in the book title face.
pub struct CoverImage {
    title: String,
    cover: Option<Pixmap>,
    /// The cover fitted to the size it was last painted at.
    scaled: Option<(Size, Pixmap)>,
}

impl CoverImage {
    pub fn new(title: impl Into<String>, cover: Option<Pixmap>) -> Self {
        Self {
            title: title.into(),
            cover,
            scaled: None,
        }
    }

    /// Replaces the picture, or removes it with `None`.
    pub fn set_cover(&mut self, cover: Option<Pixmap>) {
        self.cover = cover;
        self.scaled = None;
    }

    pub fn has_cover(&self) -> bool {
        self.cover.is_some()
    }
}

impl Widget for CoverImage {
    crate::impl_as_any!();

    /// The largest 2 by 3 box that fits the constraints.
    fn layout(&mut self, _cx: &mut LayoutCx, c: Constraints) -> Size {
        let w = c.max.w.min(c.max.h * 2 / 3);
        let h = w * 3 / 2;
        c.constrain(Size::new(w, h))
    }

    fn paint(&mut self, cx: &mut PaintCx) {
        let b = cx.bounds();
        let t = cx.theme;
        match &self.cover {
            Some(source) => {
                let stale = !matches!(&self.scaled, Some((size, _)) if *size == b.size());
                if stale {
                    self.scaled = Some((b.size(), fit_pixmap(source, b.size(), FitMode::Cover)));
                }
                if let Some((_, img)) = &self.scaled {
                    cx.blit_pixmap(img, Point::new(b.x, b.y));
                }
            }
            None => {
                let fill = cx.color(Tone::SurfaceAlt);
                cx.fill_rect(b, fill);
                let pad = t.space.sm;
                let inner = Rect::new(
                    b.x + pad as i32,
                    b.y + pad as i32,
                    b.w.saturating_sub(2 * pad).max(1),
                    b.h.saturating_sub(2 * pad).max(1),
                );
                // Small covers get a smaller face, so about six letters fit across the box.
                let mut face = t.font.book_title.clone();
                face.size = face.size.min(b.w as f32 / 6.0).max(12.0);
                let style = t.style(&face, Tone::Ink);
                let tl = cx.fonts.layout(&self.title, &style, inner.w, Some(4), true);
                let at = Point::new(inner.x, inner.y + (inner.h as i32 - tl.height as i32) / 2);
                cx.clipped(inner, |cx| cx.text(&tl, at));
            }
        }
        let ink = cx.color(Tone::Ink);
        cx.border(b, 0, t.line.hairline, ink);
    }
}

/// What a [`BookTile`] shows. `id` goes back to the app with the tap.
#[derive(Clone)]
pub struct BookInfo {
    pub id: String,
    pub title: String,
    pub author: Option<String>,
    pub cover: Option<Pixmap>,
    /// Reading progress from 0.0 to 1.0.
    pub progress: f32,
}

/// A grid cell: the cover, the title in two lines at most, the author in one
/// line, and a thin progress line. A tap queues the action set with
/// [`BookTile::on_action`] and passes the book id as its argument.
pub struct BookTile {
    id: String,
    column: Column,
    reaction: Reaction,
}

impl BookTile {
    pub fn new(theme: &Theme, book: &BookInfo) -> Self {
        let mut children = vec![
            WidgetPod::new(CoverImage::new(book.title.clone(), book.cover.clone())),
            WidgetPod::new(
                Paragraph::new(book.title.clone(), theme.font.book_title.clone(), Tone::Ink).max_lines(2),
            ),
        ];
        if let Some(author) = &book.author {
            children.push(WidgetPod::new(Label::new(
                author.clone(),
                theme.font.caption.clone(),
                Tone::InkSecondary,
            )));
        }
        children.push(WidgetPod::new(ThinBar::new(book.progress)));
        Self {
            id: book.id.clone(),
            column: Column::new(children)
                .gap(theme.space.sm)
                .cross_align(CrossAlign::Stretch)
                .shrink(),
            reaction: Reaction::None,
        }
    }

    /// Queues `id` with the book id as its argument on tap.
    pub fn on_action(mut self, id: impl Into<String>) -> Self {
        self.reaction = Reaction::Action(id.into());
        self
    }
}

impl Widget for BookTile {
    crate::impl_as_any!();

    fn layout(&mut self, cx: &mut LayoutCx, c: Constraints) -> Size {
        self.column.layout(cx, c)
    }

    fn event(&mut self, cx: &mut EventCx, ev: &UiEvent) -> Handled {
        let UiEvent::Gesture(Gesture::Tap { .. }) = ev else {
            return Handled::No;
        };
        if matches!(self.reaction, Reaction::None) {
            return Handled::No;
        }
        self.reaction.fire(cx, vec![Value::Str(self.id.clone())]);
        Handled::Yes
    }

    fn children_mut(&mut self) -> Vec<&mut WidgetPod> {
        self.column.children_mut()
    }
}

/// A thin progress line: a track in divider and a fill in the accent tone.
/// The line is `line.progress` high and takes the full width.
struct ThinBar {
    value: f32,
}

impl ThinBar {
    fn new(value: f32) -> Self {
        Self {
            value: value.clamp(0.0, 1.0),
        }
    }
}

impl Widget for ThinBar {
    crate::impl_as_any!();

    fn layout(&mut self, cx: &mut LayoutCx, c: Constraints) -> Size {
        c.constrain(Size::new(c.max.w, cx.theme.line.progress))
    }

    fn paint(&mut self, cx: &mut PaintCx) {
        let b = cx.bounds();
        let track = cx.color(Tone::Divider);
        cx.fill_rect(b, track);
        let fill_w = (b.w as f32 * self.value).round() as u32;
        let fill = cx.color(Tone::Accent);
        cx.fill_rect(Rect::new(b.x, b.y, fill_w, b.h), fill);
    }
}
