//! [`Image`]: a decoded pixmap, fitted into its box.

use kaia_gfx::image::{fit_pixmap, fit_size, FitMode};
use kaia_gfx::{Pixmap, Point, Size};

use crate::geometry::Constraints;
use crate::widget::{LayoutCx, PaintCx, Widget};

/// A pixmap drawn with [`FitMode::Contain`] (centered, whole image visible)
/// or [`FitMode::Cover`] (fills the box, edges cropped).
pub struct Image {
    source: Pixmap,
    fit: FitMode,
    /// The last scaled copy, keyed by the box size it was made for.
    scaled: Option<(Size, Pixmap)>,
}

impl Image {
    pub fn new(source: Pixmap, fit: FitMode) -> Self {
        Self {
            source,
            fit,
            scaled: None,
        }
    }

    /// Replaces the picture. The scaled copy is rebuilt on the next paint.
    /// Call [`UpdateCx::request_layout`](crate::UpdateCx::request_layout) when the
    /// aspect ratio changes and the layout must follow it.
    pub fn set_pixmap(&mut self, source: Pixmap) {
        self.source = source;
        self.scaled = None;
    }
}

impl Widget for Image {
    crate::impl_as_any!();

    fn layout(&mut self, _cx: &mut LayoutCx, c: Constraints) -> Size {
        let size = match self.fit {
            FitMode::Contain => fit_size(self.source.size(), c.max, FitMode::Contain),
            FitMode::Cover => c.max,
        };
        c.constrain(size)
    }

    fn paint(&mut self, cx: &mut PaintCx) {
        let b = cx.bounds();
        let bound = b.size();
        let stale = !matches!(&self.scaled, Some((s, _)) if *s == bound);
        if stale {
            self.scaled = Some((bound, fit_pixmap(&self.source, bound, self.fit)));
        }
        if let Some((_, img)) = &self.scaled {
            let x = b.x + (b.w as i32 - img.width() as i32) / 2;
            let y = b.y + (b.h as i32 - img.height() as i32) / 2;
            let at = match self.fit {
                FitMode::Contain => Point::new(x, y),
                FitMode::Cover => Point::new(b.x, b.y),
            };
            cx.blit_pixmap(img, at);
        }
    }
}
