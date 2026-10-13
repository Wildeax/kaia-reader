//! [`Pressable`]: a wrapper that turns a tap or a hold on its child into an
//! action. Tiles and cards take a tap and a hold this way. A child that handles
//! the tap itself (a book tile with its own action) keeps it.

use kaia_events::Value;
use kaia_gfx::Size;
use kaia_input::Gesture;
use kaia_ui::{AppAction, Constraints, EventCx, Handled, LayoutCx, UiEvent, Widget, WidgetPod};

/// An action id with its arguments.
type Act = (String, Vec<Value>);

pub struct Pressable {
    child: WidgetPod,
    tap: Option<Act>,
    hold: Option<Act>,
}

impl Pressable {
    pub fn new(child: WidgetPod) -> Self {
        Self {
            child,
            tap: None,
            hold: None,
        }
    }

    /// Queues `id` with `args` when the child is tapped and does not use the tap.
    pub fn tap(mut self, id: impl Into<String>, args: Vec<Value>) -> Self {
        self.tap = Some((id.into(), args));
        self
    }

    /// Queues `id` with `args` when the child is held.
    pub fn hold(mut self, id: impl Into<String>, args: Vec<Value>) -> Self {
        self.hold = Some((id.into(), args));
        self
    }
}

fn send(cx: &mut EventCx, act: &Act) -> Handled {
    cx.action(AppAction {
        id: act.0.clone(),
        args: act.1.clone(),
    });
    Handled::Yes
}

impl Widget for Pressable {
    kaia_ui::impl_as_any!();

    fn layout(&mut self, cx: &mut LayoutCx, c: Constraints) -> Size {
        self.child.layout(cx, c)
    }

    fn event(&mut self, cx: &mut EventCx, ev: &UiEvent) -> Handled {
        match ev {
            UiEvent::Gesture(Gesture::Tap { .. }) => match &self.tap {
                Some(act) => send(cx, act),
                None => Handled::No,
            },
            UiEvent::Gesture(Gesture::Hold { .. }) => match &self.hold {
                Some(act) => send(cx, act),
                None => Handled::No,
            },
            _ => Handled::No,
        }
    }

    fn children_mut(&mut self) -> Vec<&mut WidgetPod> {
        vec![&mut self.child]
    }
}
