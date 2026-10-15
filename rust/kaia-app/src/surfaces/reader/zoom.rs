//! PDF menu overview and zoom strip. The book worker renders the thumbnail.
use super::{chrome::rect, compact::*, state::Core};
use crate::{
    services::Services,
    surface::{Outcome, SurfaceEnv},
    surfaces::common::window,
};
use kaia_book::{BookSession, PageTransform};
use kaia_gfx::{Pixmap, Point, Rect, Size};
use kaia_i18n::{format as tr_format, tr};
use kaia_ui::{Constraints, LayoutCx, PaintCx, Tone, Widget, WidgetPod, Window};
use std::{cell::RefCell, rc::Rc, sync::Arc};
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Overview {
    pub image: Arc<Pixmap>,
    pub transform: PageTransform,
    pub percent: u32,
    pub page: u32,
    pub count: usize,
}
pub(super) fn overview(session: &mut BookSession, viewport: Size) -> Option<Overview> {
    if session.is_reflowable() {
        return None;
    }
    let transform = session.page_transform(viewport).ok()?;
    if transform.full_image.w <= viewport.w && transform.full_image.h <= viewport.h {
        return None;
    }
    let fit = (viewport.w as f32 / transform.page.width).min(viewport.h as f32 / transform.page.height);
    let image = session.render_overview(Size::new(253, 422)).ok()?;
    let info = session.current();
    Some(Overview {
        image,
        transform,
        percent: (transform.scale / fit * 100.0).round() as u32,
        page: info.page + 1,
        count: info.page_count,
    })
}
pub(super) struct Map {
    pub model: Option<Overview>,
}
impl Widget for Map {
    kaia_ui::impl_as_any!();
    fn layout(&mut self, _: &mut LayoutCx, c: Constraints) -> Size {
        c.max
    }
    fn paint(&mut self, cx: &mut PaintCx) {
        let Some(m) = &self.model else { return };
        let b = cx.bounds();
        cx.fill_rect(b, cx.color(Tone::Surface));
        cx.border(b, 0, 1, cx.color(Tone::Ink));
        let bound = Size::new(b.w * 253 / 303, b.h * 422 / 575);
        let size = kaia_gfx::image::fit_size(m.image.size(), bound, kaia_gfx::image::FitMode::Contain);
        let pix = kaia_gfx::image::scale_pixmap(&m.image, size);
        let at = Point::new(b.x + (b.w - size.w) as i32 / 2, b.y + (b.h * 25 / 575) as i32);
        cx.blit_pixmap(&pix, at);
        cx.border(Rect::new(at.x, at.y, size.w, size.h), 0, 1, cx.color(Tone::Ink));
        let t = m.transform;
        let visible = Rect::new(
            at.x + (t.offset.x as f32 * size.w as f32 / t.full_image.w as f32) as i32,
            at.y + (t.offset.y as f32 * size.h as f32 / t.full_image.h as f32) as i32,
            (t.image.w as f32 * size.w as f32 / t.full_image.w as f32).round() as u32,
            (t.image.h as f32 * size.h as f32 / t.full_image.h as f32).round() as u32,
        );
        cx.border(visible, 0, 2, cx.color(Tone::Accent));
        let mut font = cx.theme.font.body.clone();
        font.size = 30.0 * b.w as f32 / 303.0;
        let style = cx.theme.style(&font, Tone::Ink);
        for (y, text) in [
            (
                473,
                tr_format(&tr("Page %1 of %2"), &[&m.page.to_string(), &m.count.to_string()]),
            ),
            (515, tr_format(&tr("Zoom: %1%"), &[&m.percent.to_string()])),
        ] {
            let text = cx.fonts.layout(&text, &style, b.w * 270 / 303, Some(1), true);
            cx.text(
                &text,
                Point::new(b.x + (b.w * 25 / 303) as i32, b.y + (b.h * y / 575) as i32),
            );
        }
    }
}
pub(super) fn panel(services: &Services, screen: Size, model: &super::overlay::OverlayModel) -> Window {
    let mut chrome = super::overlay::chrome(services, screen, model, Some("overlay-zoom"));
    chrome.panels.push(rect(screen, 0, 179, 1264, 138));
    let percent = model.overview.as_ref().map_or(100.0, |o| o.percent as f64);
    put(
        &mut chrome,
        screen,
        [107, 200, 1028, 96],
        WidgetPod::new(
            crate::slider::Slider::new(percent, 100.0, 400.0, 10.0)
                .reader()
                .on_action("reader:zoom:percent"),
        )
        .with_key("zoom-slider"),
    );
    put(
        &mut chrome,
        screen,
        [24, 204, 88, 88],
        action(screen, "−", "reader:zoom:minus", "zoom-minus"),
    );
    put(
        &mut chrome,
        screen,
        [1151, 204, 88, 88],
        action(screen, "+", "reader:zoom:plus", "zoom-plus"),
    );
    put(
        &mut chrome,
        screen,
        [42, 340, 303, 575],
        WidgetPod::new(Map {
            model: model.overview.clone(),
        })
        .with_key("zoom-map"),
    );
    window(services, screen, chrome)
}
pub(super) fn command(
    rc: &Rc<RefCell<Core>>,
    cmd: &str,
    args: &[kaia_events::Value],
    env: &mut SurfaceEnv<'_>,
) -> Option<Outcome> {
    if cmd == "reader:zoom:fit-modes" {
        let mut c = rc.borrow_mut();
        let zoom = c.session.as_ref()?.zoom();
        let window = super::panels::layout(env.services, env.screen, None, zoom);
        return Some(super::show_panel(rc, &mut c, env.ctx, window, true));
    }
    if cmd == "reader:zoom:percent:release" {
        return Some(Outcome::Handled);
    }
    if !matches!(
        cmd,
        "reader:zoom:percent" | "reader:zoom:minus" | "reader:zoom:plus"
    ) {
        return None;
    }
    let mut c = rc.borrow_mut();
    let viewport = c.viewport;
    let session = c.session.as_mut()?;
    if session.is_reflowable() {
        return Some(Outcome::Handled);
    }
    let t = session.page_transform(viewport).ok()?;
    let current = t.scale / (viewport.w as f32 / t.page.width).min(viewport.h as f32 / t.page.height) * 100.0;
    let value = match cmd {
        "reader:zoom:minus" => current - 10.0,
        "reader:zoom:plus" => current + 10.0,
        _ => args.first()?.as_f64()? as f32,
    };
    if !value.is_finite() {
        return Some(Outcome::Handled);
    }
    let center = Point::new(viewport.w as i32 / 2, viewport.h as i32 / 2);
    if let Err(e) = session.pinch_zoom(value.clamp(100.0, 400.0) / current, center, center, viewport) {
        env.toast(e.to_string(), 3.0);
    }
    c.repaint_page(env.ctx, kaia_ui::widget::PaintHint::Ui);
    Some(Outcome::Handled)
}
