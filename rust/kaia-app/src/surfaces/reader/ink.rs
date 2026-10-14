//! Stored page ink and the live stroke. Engine calls happen when the page loads.
use std::path::PathBuf;

use kaia_book::{BookSession, Location, PageTransform};
use kaia_gfx::{Color, Point, Rect, Size};
use kaia_input::{PenEvent, Phase};
use kaia_storage::{DocumentId, InkPoint, InkStroke, Store};
use kaia_ui::widget::{PaintCx, PaintHint};

#[derive(Clone)]
pub struct Drawing {
    pub stroke: InkStroke,
    origin: (f32, f32),
}

pub struct PageInk {
    dir: PathBuf,
    doc: DocumentId,
    page: u32,
    locator: Option<String>,
    origin: (f32, f32),
    transform: PageTransform,
    offset: Point,
    bounds: Rect,
    pub strokes: Vec<Drawing>,
    active: Option<Drawing>,
    erasing: bool,
    previous: Option<Point>,
    erased: Vec<i64>,
    dirty: Option<Rect>,
}

pub(super) fn frame(session: &mut BookSession, viewport: Size) -> Result<PageInk, String> {
    let store = super::stickers::store(session)?;
    let doc = super::stickers::identity(session);
    let transform = session.page_transform(viewport).map_err(|e| e.to_string())?;
    let info = session.current();
    let locator = match info.location {
        Location::Engine(xp) => Some(xp),
        Location::Page(_) => None,
    };
    // A reflowable engine without text positions cannot safely attach ink.
    if session.is_reflowable() && locator.is_none() {
        return Err("This book engine has no text positions for ink.".into());
    }
    // The page origin is eager. Frames cannot see the ink mode, so a lazy origin
    // needs the pen path in page.rs and the state in state.rs.
    let origin = match &locator {
        Some(xp) => session
            .ink_anchor(xp)
            .map_err(|e| e.to_string())?
            .map(|(_, p)| (p.x, p.y))
            .unwrap_or((0.0, 0.0)),
        None => (0.0, 0.0),
    };
    let mut strokes = Vec::new();
    // A book without strokes has no stroke work at all.
    if store.ink_count(&doc.partial_md5).map_err(|e| e.to_string())? > 0 {
        let stored = store.ink(&doc.partial_md5).map_err(|e| e.to_string())?;
        // One engine call resolves every stroke with a locator. The results follow
        // the order of `located`, which lists the same strokes in the same order.
        let located: Vec<String> = stored.iter().filter_map(|s| s.locator.clone()).collect();
        let mut origins = session
            .ink_origins(info.page, &located)
            .map_err(|e| e.to_string())?
            .into_iter();
        for stroke in stored {
            let origin = match &stroke.locator {
                Some(_) => origins.next().flatten().map(|p| (p.x, p.y)),
                None => (stroke.page == info.page).then_some((0.0, 0.0)),
            };
            if let Some(origin) = origin {
                strokes.push(Drawing { stroke, origin });
            }
        }
    }
    let offset = Point::new(
        (viewport.w as i32 - transform.image.w as i32) / 2,
        (viewport.h as i32 - transform.image.h as i32) / 2,
    );
    Ok(PageInk {
        dir: store.dir().to_path_buf(),
        doc,
        page: info.page,
        locator,
        origin,
        transform,
        offset,
        bounds: Rect::new(offset.x, offset.y, transform.image.w, transform.image.h)
            .intersect(&Rect::from_size(viewport)),
        strokes,
        active: None,
        erasing: false,
        previous: None,
        erased: Vec::new(),
        dirty: None,
    })
}

impl PageInk {
    fn point(&self, point: &InkPoint, origin: (f32, f32)) -> Point {
        Point::new(
            self.offset.x + ((point.x + origin.0) * self.transform.scale).round() as i32,
            self.offset.y + ((point.y + origin.1) * self.transform.scale).round() as i32,
        )
    }

    fn width(&self, stroke: &InkStroke) -> f32 {
        (stroke.width * self.transform.scale).max(1.0)
    }

    fn rect(&self, drawing: &Drawing) -> Rect {
        drawing
            .stroke
            .points
            .iter()
            .map(|p| {
                let p = self.point(p, drawing.origin);
                segment_rect(p, p, self.width(&drawing.stroke))
            })
            .reduce(|a, b| a.union(&b))
            .unwrap_or_default()
            .intersect(&self.bounds)
    }

    /// The page image area on screen that strokes map to.
    pub fn bounds(&self) -> Rect {
        self.bounds
    }

    /// Moves the page down by `dy` pixels on screen. The reader calls it with the
    /// header height, since the page image sits below the header strip.
    pub(super) fn shift_down(&mut self, dy: i32) {
        self.offset.y += dy;
        self.bounds.y += dy;
    }

    pub fn paint(&self, cx: &mut PaintCx) {
        cx.clipped(self.bounds, |cx| {
            for drawing in self
                .strokes
                .iter()
                .filter(|d| !self.erased.contains(&d.stroke.id))
                .chain(self.active.iter())
            {
                // A2 needs a black preview. Lift saves the stroke and restores its color.
                let color = if cx.theme.color_panel && drawing.stroke.id != 0 {
                    kaia_book::highlight_rgb(&drawing.stroke.color)
                        .map(|[r, g, b]| Color::rgb(r, g, b))
                        .unwrap_or(Color::BLACK)
                } else {
                    Color::BLACK
                };
                let points: Vec<_> = drawing
                    .stroke
                    .points
                    .iter()
                    .map(|p| self.point(p, drawing.origin))
                    .collect();
                let width = self.width(&drawing.stroke);
                if let Some(p) = points.first() {
                    cx.fill_circle(*p, width / 2.0, color);
                }
                for pair in points.windows(2) {
                    paint_segment(cx, pair[0], pair[1], width, color);
                }
            }
        });
    }

    /// Live samples change only their dirty rectangle. Disk writes wait for lift.
    pub fn pen(
        &mut self,
        pen: PenEvent,
        width: f32,
        color: &str,
        eraser: bool,
    ) -> Result<Option<(Rect, PaintHint)>, String> {
        if pen.phase == Phase::Down {
            if !self.bounds.contains(pen.pos) {
                return Ok(None);
            }
            self.active = None;
            self.erased.clear();
            self.dirty = None;
            self.previous = Some(pen.pos);
            self.erasing = eraser || pen.eraser || pen.button;
            if !self.erasing {
                self.active = Some(Drawing {
                    origin: self.origin,
                    stroke: InkStroke {
                        id: 0,
                        page: self.page,
                        locator: self.locator.clone(),
                        points: Vec::new(),
                        width: width / self.transform.scale,
                        color: color.to_string(),
                    },
                });
            }
        }
        let Some(previous) = self.previous else {
            return Ok(None);
        };
        let p = Point::new(
            pen.pos.x.clamp(self.bounds.x, self.bounds.right() - 1),
            pen.pos.y.clamp(self.bounds.y, self.bounds.bottom() - 1),
        );
        let mut dirty = segment_rect(previous, p, width).intersect(&self.bounds);
        if self.erasing {
            for drawing in &self.strokes {
                if self.erased.contains(&drawing.stroke.id) {
                    continue;
                }
                let points: Vec<_> = drawing
                    .stroke
                    .points
                    .iter()
                    .map(|q| self.point(q, drawing.origin))
                    .collect();
                let radius = 12.0 + self.width(&drawing.stroke) / 2.0;
                let hit = points
                    .first()
                    .is_some_and(|q| distance_to_segment(*q, previous, p) <= radius)
                    || points
                        .windows(2)
                        .any(|q| segments_touch(previous, p, q[0], q[1], radius));
                if hit {
                    dirty = dirty.union(&self.rect(drawing));
                    self.erased.push(drawing.stroke.id);
                }
            }
        } else if let Some(active) = &mut self.active {
            if active.stroke.points.len() < 100_000 {
                active.stroke.points.push(InkPoint {
                    x: (p.x - self.offset.x) as f32 / self.transform.scale - active.origin.0,
                    y: (p.y - self.offset.y) as f32 / self.transform.scale - active.origin.1,
                    pressure: pen.pressure,
                });
            }
            dirty =
                segment_rect(previous, p, active.stroke.width * self.transform.scale).intersect(&self.bounds);
        }
        self.previous = Some(p);
        self.dirty = Some(self.dirty.map_or(dirty, |r| r.union(&dirty)));
        if pen.phase == Phase::Up {
            self.previous = None;
            let result = self.save();
            let rect = self.dirty.take().unwrap_or(dirty);
            if let Err(error) = result {
                self.active = None;
                self.erased.clear();
                return Err(error);
            }
            return Ok(Some((rect, PaintHint::Partial)));
        }
        Ok(Some((dirty, PaintHint::A2)))
    }

    fn save(&mut self) -> Result<(), String> {
        let mut store = Store::open(&self.dir).map_err(|e| e.to_string())?;
        if self.erasing {
            store
                .delete_ink(&self.doc.partial_md5, &self.erased)
                .map_err(|e| e.to_string())?;
            self.strokes.retain(|d| !self.erased.contains(&d.stroke.id));
            self.erased.clear();
        } else if let Some(active) = &mut self.active {
            active.stroke.id = store
                .add_ink(&self.doc, &active.stroke)
                .map_err(|e| e.to_string())?;
            self.strokes.push(self.active.take().unwrap());
        }
        Ok(())
    }

    pub fn cancel(&mut self) {
        self.active = None;
        self.erased.clear();
        self.previous = None;
        self.dirty = None;
    }

    pub fn delete(&mut self, all: bool) -> Result<(), String> {
        let ids: Vec<_> = if all {
            self.strokes.iter().map(|d| d.stroke.id).collect()
        } else {
            self.strokes.last().map(|d| vec![d.stroke.id]).unwrap_or_default()
        };
        Store::open(&self.dir)
            .and_then(|mut s| s.delete_ink(&self.doc.partial_md5, &ids))
            .map_err(|e| e.to_string())?;
        self.strokes.retain(|d| !ids.contains(&d.stroke.id));
        Ok(())
    }
}

fn segment_rect(a: Point, b: Point, width: f32) -> Rect {
    let pad = (width / 2.0).ceil() as i32 + 1;
    Rect::new(
        a.x.min(b.x) - pad,
        a.y.min(b.y) - pad,
        (a.x - b.x).unsigned_abs() + 2 * pad as u32 + 1,
        (a.y - b.y).unsigned_abs() + 2 * pad as u32 + 1,
    )
}

fn paint_segment(cx: &mut PaintCx, a: Point, b: Point, width: f32, color: Color) {
    if segment_rect(a, b, width).intersect(&cx.clip()).is_empty() {
        return;
    }
    let steps = (a.x - b.x).unsigned_abs().max((a.y - b.y).unsigned_abs()).max(1);
    for i in 0..=steps {
        let t = i as f32 / steps as f32;
        cx.fill_circle(
            Point::new(
                (a.x as f32 + (b.x - a.x) as f32 * t).round() as i32,
                (a.y as f32 + (b.y - a.y) as f32 * t).round() as i32,
            ),
            width / 2.0,
            color,
        );
    }
}

fn distance_to_segment(p: Point, a: Point, b: Point) -> f32 {
    let (x, y) = ((b.x - a.x) as f32, (b.y - a.y) as f32);
    let t = if x * x + y * y == 0.0 {
        0.0
    } else {
        (((p.x - a.x) as f32 * x + (p.y - a.y) as f32 * y) / (x * x + y * y)).clamp(0.0, 1.0)
    };
    ((p.x as f32 - a.x as f32 - t * x).powi(2) + (p.y as f32 - a.y as f32 - t * y).powi(2)).sqrt()
}

fn segments_touch(a: Point, b: Point, c: Point, d: Point, radius: f32) -> bool {
    let cross = |p: Point, q: Point, r: Point| {
        (q.x - p.x) as f64 * (r.y - p.y) as f64 - (q.y - p.y) as f64 * (r.x - p.x) as f64
    };
    let intersects = cross(a, b, c) * cross(a, b, d) < 0.0 && cross(c, d, a) * cross(c, d, b) < 0.0;
    intersects
        || distance_to_segment(a, c, d)
            .min(distance_to_segment(b, c, d))
            .min(distance_to_segment(c, a, b))
            .min(distance_to_segment(d, a, b))
            <= radius
}

pub(super) fn toolbar(theme: &kaia_ui::Theme) -> kaia_ui::WidgetPod {
    use crate::surfaces::common::button;
    use kaia_ui::components::{Frame, Row};
    use kaia_ui::controls::ButtonStyle;
    let mut children = vec![button(
        ButtonStyle::Text,
        &kaia_i18n::tr("Thin"),
        "reader:ink:width",
        "ink-width",
    )];
    if theme.color_panel {
        children.push(button(
            ButtonStyle::Text,
            &kaia_i18n::tr("Black"),
            "reader:ink:color",
            "ink-color",
        ));
    }
    // Literal tr() calls, so the string extractor finds each label.
    for (label, action, key) in [
        (kaia_i18n::tr("Eraser"), "reader:ink:eraser", "ink-eraser"),
        (kaia_i18n::tr("Undo"), "reader:ink:undo", "ink-undo"),
        (kaia_i18n::tr("Delete"), "reader:ink:delete", "ink-delete"),
        (kaia_i18n::tr("Done"), "reader:ink:toggle", "ink-done"),
    ] {
        children.push(button(ButtonStyle::Text, &label, action, key));
    }
    kaia_ui::WidgetPod::new(
        Frame::new(kaia_ui::WidgetPod::new(
            Row::new(children).gap(theme.space.xs).shrink(),
        ))
        .fill(kaia_ui::Tone::Surface)
        .border(kaia_ui::Tone::Divider, theme.line.divider),
    )
}

pub(super) fn command(
    rc: &std::rc::Rc<std::cell::RefCell<super::state::Core>>,
    cmd: &str,
    args: &[kaia_events::Value],
    env: &mut crate::surface::SurfaceEnv<'_>,
) -> Option<crate::surface::Outcome> {
    use crate::surface::Outcome;
    use kaia_i18n::{format as tr_format, tr};
    use kaia_ui::Widget;
    let action = cmd.strip_prefix("reader:ink:")?;
    if action == "error" {
        env.notice(tr_format(
            &tr("Cannot save ink. %1"),
            &[args.first().and_then(|v| v.as_str()).unwrap_or("")],
        ));
        return Some(Outcome::Handled);
    }
    let mut c = rc.borrow_mut();
    if action == "pencil-bookmark" {
        if let Some(session) = c.session.as_mut() {
            if let Err(e) = session.set_ink_bookmark(args.first().and_then(|v| v.as_bool()).unwrap_or(false))
            {
                env.notice(e.to_string());
            }
        }
        return Some(Outcome::Handled);
    }
    let window = c.page_window.clone()?;
    if action == "delete" {
        use kaia_ui::composite::{Dialog, DialogButton};
        env.dialog(Dialog::new(
            env.theme(),
            tr("Delete all ink on this page?"),
            tr("This removes every ink stroke on this page. You cannot undo this deletion."),
            vec![
                DialogButton::new(tr("Cancel"), "dialog:dismiss"),
                DialogButton::new(tr("Delete"), "reader:ink:delete-confirm"),
            ],
        ));
        return Some(Outcome::Handled);
    }
    let mut error = None;
    let mut on = false;
    let mut color = String::new();
    let mut tool = String::new();
    let mut width_label = String::new();
    let mut color_content = false;
    let mut pencil_bookmark = None;
    window.update::<super::page::PageView, _>(env.ctx, super::page::PAGE_KEY, |view, cx| {
        match action {
            "pencil-on" | "pencil-off" => {
                view.ink_pencil = action == "pencil-on";
                view.ink_on = view.ink_pencil;
                if !view.ink_on {
                    if let Some(ink) = &mut view.ink {
                        ink.cancel();
                    }
                }
            }
            "pencil-pen" => view.ink_eraser = false,
            "pencil-eraser" => view.ink_eraser = true,
            "toggle" => {
                view.ink_on = !view.ink_on;
                if !view.ink_on {
                    if let Some(ink) = &mut view.ink {
                        ink.cancel();
                    }
                }
            }
            "width" => {
                view.ink_width = if view.ink_width == 3.0 { 7.0 } else { 3.0 };
                view.ink_eraser = false;
            }
            "thin" => {
                view.ink_width = 3.0;
                view.ink_eraser = false;
            }
            "thick" => {
                view.ink_width = 7.0;
                view.ink_eraser = false;
            }
            "eraser" => view.ink_eraser = !view.ink_eraser,
            "color" if c.color_panel => {
                view.ink_color = match view.ink_color.as_str() {
                    "black" => "blue",
                    "blue" => "green",
                    "green" => "red",
                    _ => "black",
                }
                .into();
                view.ink_eraser = false;
            }
            "undo" | "delete-confirm" => {
                if let Some(ink) = &mut view.ink {
                    error = ink.delete(action == "delete-confirm").err();
                    if error.is_none() && view.ink_pencil {
                        pencil_bookmark = Some(!ink.strokes.is_empty());
                    }
                }
            }
            _ => {}
        }
        on = view.ink_on;
        color_content = view.ink_color != "black";
        width_label = if view.ink_width == 3.0 {
            tr("Thin")
        } else {
            tr("Thick")
        };
        color = match view.ink_color.as_str() {
            "blue" => tr("Blue"),
            "green" => tr("Green"),
            "red" => tr("Red"),
            _ => tr("Black"),
        };
        tool = if view.ink_eraser {
            tr("Eraser on")
        } else {
            tr("Eraser")
        };
        cx.request_paint();
    });
    if let (Some(active), Some(session)) = (pencil_bookmark, c.session.as_mut()) {
        error = session.set_ink_bookmark(active).err().map(|e| e.to_string());
    }
    c.ink_color_content |= color_content;
    if matches!(action, "toggle" | "pencil-on" | "pencil-off") {
        window.update::<kaia_ui::components::Stack, _>(env.ctx, "ink-root", |root, cx| {
            root.children_mut()[1].set_visible(on);
            cx.request_layout();
            cx.request_paint();
        });
        for layer in [c.panel.take(), c.overlay.take()].into_iter().flatten() {
            if let Some(id) = layer.window.id() {
                env.ctx.close(id, None);
            }
        }
    }
    for (key, label) in [
        ("ink-color", color),
        ("ink-eraser", tool),
        ("ink-width", width_label),
    ] {
        window.update::<kaia_ui::controls::Button, _>(env.ctx, key, |button, cx| {
            button.set_text(label);
            cx.request_layout();
            cx.request_paint();
        });
    }
    if let Some(e) = error {
        env.notice(tr_format(&tr("Cannot save ink. %1"), &[&e]));
    }
    Some(Outcome::Handled)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kaia_book::OpenOptions;
    use kaia_storage::{InkPoint, InkStroke};
    use std::path::Path;

    const VIEW: Size = Size { w: 1264, h: 1680 };

    fn juliet() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test/juliet.epub")
    }

    fn open(dir: &tempfile::TempDir) -> BookSession {
        let mut opts = OpenOptions::new(dir.path().join("data"));
        opts.viewport = VIEW;
        BookSession::open(juliet(), opts).expect("open juliet")
    }

    fn stroke(page: u32, locator: Option<String>) -> InkStroke {
        InkStroke {
            id: 0,
            page,
            locator,
            points: vec![
                InkPoint {
                    x: 10.0,
                    y: 10.0,
                    pressure: 500,
                },
                InkPoint {
                    x: 40.0,
                    y: 60.0,
                    pressure: 500,
                },
            ],
            width: 3.0,
            color: "black".into(),
        }
    }

    #[test]
    fn a_book_without_strokes_has_no_stroke_drawings_on_any_page() {
        let dir = tempfile::tempdir().unwrap();
        let mut session = open(&dir);
        for page in [0, 1, 5] {
            session.go_to(Location::Page(page)).expect("go to page");
            let ink = frame(&mut session, VIEW).expect("frame");
            assert!(ink.strokes.is_empty(), "page {page} has no ink");
        }
    }

    #[test]
    fn stored_strokes_show_on_their_page_and_nowhere_else() {
        let dir = tempfile::tempdir().unwrap();
        let mut session = open(&dir);
        session.go_to(Location::Page(2)).expect("go to page");
        let Location::Engine(locator) = session.current().location else {
            panic!("juliet has text positions");
        };
        let (_, expected) = session
            .ink_anchor(&locator)
            .expect("anchor")
            .expect("locator in document");
        let doc = super::super::stickers::identity(&session);
        let mut store = Store::open(&dir.path().join("data/kaia")).unwrap();
        store
            .add_ink(&doc, &stroke(2, Some(locator.clone())))
            .expect("save text stroke");
        store
            .add_ink(&doc, &stroke(2, None))
            .expect("save fixed stroke on the same page number");
        drop(store);

        let ink = frame(&mut session, VIEW).expect("frame on page 2");
        assert_eq!(ink.strokes.len(), 2, "both strokes are on page 2");
        assert_eq!(ink.strokes[0].origin, (expected.x, expected.y));
        assert_eq!(ink.strokes[1].origin, (0.0, 0.0));

        session.go_to(Location::Page(3)).expect("go to page");
        let ink = frame(&mut session, VIEW).expect("frame on page 3");
        assert!(ink.strokes.is_empty(), "page 3 shows no page 2 strokes");
    }
}
