//! [`Dialog`] and [`Toast`]: short messages on their own window.

use kaia_events::{Ctx, ShowOpts};
use kaia_gfx::{Point, Size};

use crate::assets::SharedAssets;
use crate::components::{Column, CrossAlign, Frame, Label, MainAlign, Padding, Paragraph, Row};
use crate::controls::{Button, ButtonStyle};
use crate::geometry::{Constraints, Insets};
use crate::pod::WidgetPod;
use crate::theme::{Theme, Tone};
use crate::widget::{EventCx, Handled, LayoutCx, PaintCx, UiEvent, Widget};
use crate::window::Window;

/// One button of a dialog: its label and the action it queues.
pub struct DialogButton {
    pub label: String,
    pub action: String,
}

impl DialogButton {
    pub fn new(label: impl Into<String>, action: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            action: action.into(),
        }
    }
}

/// A modal panel with a title, a body paragraph and up to three buttons. The
/// last button is the main action: it is primary and sits at the right edge.
///
/// Each button is keyed `dialog-button:<action>` for [`Window::find_rect`](crate::Window::find_rect).
///
/// The panel is centered on a dimmed scrim. A tap on the scrim does nothing,
/// so the user leaves through a button. The window the dialog runs in must be
/// modal, which [`Dialog::show`] takes care of.
pub struct Dialog {
    panel: WidgetPod,
}

impl Dialog {
    /// Panics when there are more than three buttons.
    pub fn new(
        theme: &Theme,
        title: impl Into<String>,
        body: impl Into<String>,
        buttons: Vec<DialogButton>,
    ) -> Self {
        assert!(buttons.len() <= 3, "a dialog holds three buttons at most");
        let count = buttons.len();
        let row_buttons: Vec<WidgetPod> = buttons
            .into_iter()
            .enumerate()
            .map(|(i, b)| {
                let style = if i + 1 == count {
                    ButtonStyle::Primary
                } else {
                    ButtonStyle::Secondary
                };
                let key = format!("dialog-button:{}", b.action);
                WidgetPod::new(Button::new(style).text(b.label).on_action(b.action)).with_key(key)
            })
            .collect();
        let t = theme;
        let title = WidgetPod::new(Label::new(title, t.font.title.clone(), Tone::Ink));
        let body = WidgetPod::new(Paragraph::new(body, t.font.body.clone(), Tone::Ink));
        let mut content = vec![title, body];
        if !row_buttons.is_empty() {
            content.push(WidgetPod::new(
                Row::new(row_buttons)
                    .gap(t.space.md)
                    .main_align(MainAlign::End)
                    .cross_align(CrossAlign::Center),
            ));
        }
        let column = Column::new(content)
            .gap(t.space.md)
            .cross_align(CrossAlign::Stretch)
            .shrink();
        let padded = WidgetPod::new(Padding::new(WidgetPod::new(column), Insets::all(t.space.lg)));
        let panel = Frame::new(padded)
            .fill(Tone::Surface)
            .border(Tone::Ink, t.line.hairline)
            .radius(t.radius.card);
        Self {
            panel: WidgetPod::new(panel),
        }
    }

    /// The options for a dialog window: modal, and drawn over the windows below.
    pub fn show_opts() -> ShowOpts {
        ShowOpts {
            modal: true,
            ..ShowOpts::default()
        }
    }

    /// Queues the dialog on `ctx` as a modal window and returns the window
    /// handle. Close it with `ctx.close(window.id())`.
    pub fn show(self, ctx: &mut Ctx, theme: &Theme, assets: &SharedAssets, screen: Size) -> Window {
        let window = Window::new(self, theme.clone(), assets.clone(), screen);
        window.show(ctx, Self::show_opts());
        window
    }
}

impl Widget for Dialog {
    crate::impl_as_any!();

    /// The panel takes four fifths of the width and centers on the screen.
    fn layout(&mut self, cx: &mut LayoutCx, c: Constraints) -> Size {
        let w = c.max.w * 4 / 5;
        let s = self.panel.layout(
            cx,
            Constraints {
                min: Size::new(w, 0),
                max: Size::new(w, c.max.h),
            },
        );
        let x = (c.max.w.saturating_sub(s.w) / 2) as i32;
        let y = (c.max.h.saturating_sub(s.h) / 2) as i32;
        self.panel.set_origin(Point::new(x, y));
        c.constrain(c.max)
    }

    fn paint(&mut self, cx: &mut PaintCx) {
        cx.dim(cx.bounds(), 96);
    }

    /// Gestures on the scrim stop here, so they never reach the window below.
    fn event(&mut self, _cx: &mut EventCx, ev: &UiEvent) -> Handled {
        match ev {
            UiEvent::Gesture(_) => Handled::Yes,
            _ => Handled::No,
        }
    }

    fn children_mut(&mut self) -> Vec<&mut WidgetPod> {
        vec![&mut self.panel]
    }
}
