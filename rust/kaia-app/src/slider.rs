//! App sliders use Fast during a drag and finish with Ui on release.
use kaia_events::Value;
use kaia_gfx::Size;
use kaia_input::{Gesture, Key};
use kaia_ui::geometry::Constraints;
use kaia_ui::widget::{AppAction, EventCx, Handled, LayoutCx, PaintCx, PaintHint, UiEvent, Widget};

pub struct Slider {
    reader: bool,
    inner: kaia_ui::controls::Slider,
    min: f64,
    max: f64,
    step: f64,
    enabled: bool,
    action: String,
    dragging: bool,
    format: Box<dyn Fn(f64) -> String>,
}
impl Slider {
    pub fn new(value: f64, min: f64, max: f64, step: f64) -> Self {
        Self {
            reader: false,
            inner: kaia_ui::controls::Slider::new(value, min, max, step),
            min,
            max,
            step,
            enabled: true,
            action: String::new(),
            dragging: false,
            format: Box::new(|v| format!("{v}")),
        }
    }
    pub fn reader(mut self) -> Self {
        self.reader = true;
        self
    }
    pub fn format(mut self, f: impl Fn(f64) -> String + Clone + 'static) -> Self {
        self.format = Box::new(f.clone());
        self.inner = self.inner.format(f);
        self
    }
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self.inner.set_enabled(enabled);
        self
    }
    pub fn on_action(mut self, id: impl Into<String>) -> Self {
        self.action = id.into();
        self
    }
    pub fn value(&self) -> f64 {
        self.inner.value()
    }
    pub fn set_value(&mut self, value: f64) {
        self.inner.set_value(value);
    }
    fn change(&mut self, cx: &mut EventCx, value: f64, hint: PaintHint) {
        let old = self.inner.value();
        self.inner.set_value(value);
        if old != self.inner.value() {
            cx.request_paint_with(hint);
            cx.action(AppAction {
                id: self.action.clone(),
                args: vec![Value::Number(self.inner.value()), Value::Bool(self.dragging)],
            });
        }
    }
    fn at(&mut self, cx: &mut EventCx, x: i32, hint: PaintHint) {
        if self.reader {
            let b = cx.bounds();
            let pad = (b.h / 4).min(b.w / 4);
            let fraction = f64::from(x - b.x - pad as i32) / f64::from(b.w.saturating_sub(2 * pad).max(1));
            self.change(
                cx,
                self.min + fraction.clamp(0.0, 1.0) * (self.max - self.min),
                hint,
            );
            return;
        }
        let t = cx.theme();
        let style = t.style(&t.font.body, kaia_ui::Tone::Ink);
        let lo = cx
            .fonts()
            .layout(&(self.format)(self.min), &style, 1 << 20, Some(1), false)
            .width;
        let hi = cx
            .fonts()
            .layout(&(self.format)(self.max), &style, 1 << 20, Some(1), false)
            .width;
        let b = cx.bounds();
        let start = b.x + (t.line.focus + t.space.md) as i32;
        let end = b.right() - (t.line.focus + 2 * t.space.md + lo.max(hi)) as i32;
        let fraction = f64::from(x - start) / f64::from((end - start).max(1));
        self.change(
            cx,
            self.min + fraction.clamp(0.0, 1.0) * (self.max - self.min),
            hint,
        );
    }
}
impl Widget for Slider {
    kaia_ui::impl_as_any!();
    fn layout(&mut self, cx: &mut LayoutCx, c: Constraints) -> Size {
        self.inner.layout(cx, c)
    }
    fn paint(&mut self, cx: &mut PaintCx) {
        if !self.reader {
            self.inner.paint(cx);
            return;
        }
        let b = cx.bounds();
        let pad = (b.h / 4).min(b.w / 4);
        let width = b.w.saturating_sub(2 * pad);
        let fraction = if self.max > self.min {
            (self.value() - self.min) / (self.max - self.min)
        } else {
            0.0
        };
        let x = b.x + pad as i32;
        let y = b.y + b.h as i32 / 2;
        let ink = cx.color(kaia_ui::Tone::Ink);
        let fill = (fraction * f64::from(width)) as u32;
        cx.fill_rect(kaia_gfx::Rect::new(x, y, width, 1), ink);
        cx.fill_rect(kaia_gfx::Rect::new(x, y - 2, fill, 5), ink);
        let center = kaia_gfx::Point::new(x + fill as i32, y);
        let radius = pad as f32;
        cx.fill_circle(center, radius, cx.color(kaia_ui::Tone::Surface));
        cx.stroke_circle(center, radius, 2.0, ink);
        cx.stroke_circle(center, (radius - 7.0).max(1.0), 1.0, ink);
    }
    fn event(&mut self, cx: &mut EventCx, ev: &UiEvent) -> Handled {
        if !self.enabled {
            return Handled::No;
        }
        match ev {
            UiEvent::Gesture(Gesture::Pan { pos, .. }) => {
                self.dragging = true;
                self.at(cx, pos.x, PaintHint::Fast);
            }
            UiEvent::Gesture(Gesture::PanRelease { .. }) => {
                if self.dragging {
                    self.dragging = false;
                    cx.request_paint_with(PaintHint::Ui);
                    cx.action(AppAction {
                        id: format!("{}:release", self.action),
                        args: vec![Value::Number(self.inner.value())],
                    });
                }
            }
            UiEvent::Gesture(Gesture::Tap { pos, .. }) => {
                cx.request_focus_if_focusable();
                self.at(cx, pos.x, PaintHint::Ui);
            }
            UiEvent::Gesture(Gesture::Swipe { end_pos, .. }) => {
                let dragging = std::mem::take(&mut self.dragging);
                self.at(cx, end_pos.x, PaintHint::Ui);
                if dragging {
                    cx.request_paint();
                    cx.action(AppAction {
                        id: format!("{}:release", self.action),
                        args: vec![Value::Number(self.inner.value())],
                    });
                }
            }
            UiEvent::Key(k) if k.pressed && cx.is_focused() => {
                let step = match k.key {
                    Key::Left | Key::Down => -self.step,
                    Key::Right | Key::Up => self.step,
                    _ => return Handled::No,
                };
                self.change(cx, self.inner.value() + step, PaintHint::Ui);
            }
            _ => return Handled::No,
        }
        Handled::Yes
    }
}
