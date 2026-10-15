//! The screen lock. With a PIN set, the app shows a full-screen pad after the
//! device wakes and when the app starts. The pad covers the screens below it, and
//! only the right PIN closes it. Settings uses the same pad to set, change and turn
//! off the PIN.
//!
//! [`store`] holds the files, [`session`] the flows and the wrong-try rule.
//! [`LockSurface`] is the lock on the stack. `settings` pushes no pad of its own:
//! its device section shows the pad in place of its rows.

pub mod session;
pub mod store;

use std::path::Path;

use kaia_events::{Event, Refresh, RefreshMode, ShowOpts, Value};
use kaia_gfx::Size;
use kaia_i18n::{format as tr_format, tr, tr_n};
use kaia_ui::components::{Align, Column, Expanded, Frame, Label, Paragraph, Row, SizedBox};
use kaia_ui::composite::Header;
use kaia_ui::{Alignment, FontSpec, Tone, WidgetPod, Window};

use crate::services::Services;
use crate::surface::{Nav, Outcome, Surface, SurfaceEnv};
use crate::surfaces::common::{page, root_window, title};
use crate::surfaces::library::press::Pressable;
use crate::surfaces::Route;
use session::{Done, Notice, Purpose, Session, Step};

/// The name the lock's route carries. The `Route` enum has no variant for the lock,
/// so the lock uses a `SettingsSection` route with this name, as the storage page does.
const LOCK_ROUTE: &str = "screen-lock";

/// True for the route of the lock.
pub fn is_lock_route(route: &Route) -> bool {
    matches!(route, Route::SettingsSection { name } if name == LOCK_ROUTE)
}

/// The lock on the stack. It holds the session and answers the pad's actions. Back
/// cannot close it (see `App::back`). Only the right PIN pops it.
pub struct LockSurface {
    session: Session,
}

impl LockSurface {
    /// The lock for the PIN in `dir`. It starts with any wait that still runs.
    pub fn new(dir: &Path) -> Self {
        Self {
            session: Session::new(dir, Purpose::Unlock, session::unix_now()),
        }
    }
}

impl Surface for LockSurface {
    fn route(&self) -> Route {
        Route::SettingsSection {
            name: LOCK_ROUTE.into(),
        }
    }

    fn show_opts(&self) -> ShowOpts {
        ShowOpts {
            modal: true,
            covers_fullscreen: true,
            refresh: Some(Refresh::new(RefreshMode::Full)),
            ..ShowOpts::default()
        }
    }

    fn build(&mut self, env: &mut SurfaceEnv<'_>) -> Option<Window> {
        Some(view(env.services, env.screen, &self.session, false))
    }

    fn on_command(&mut self, cmd: &str, _args: &[Value], env: &mut SurfaceEnv<'_>) -> Outcome {
        match act(&mut self.session, cmd, session::unix_now()) {
            None => Outcome::NotMine,
            Some(Ok(Some(Done::Unlocked))) => Outcome::Navigate(Nav::Pop),
            Some(Ok(_)) => Outcome::Rebuild,
            Some(Err(error)) => {
                env.notice(tr_format(&tr("Cannot save the PIN: %1"), &[&error.to_string()]));
                Outcome::Rebuild
            }
        }
    }

    /// A wake reuses the lock when it is still on top. It drops the digits typed
    /// before the sleep and reads the wrong tries again.
    fn on_event(&mut self, ev: &Event, _env: &mut SurfaceEnv<'_>) -> Outcome {
        if ev.name == "Resume" {
            self.session.reload(session::unix_now());
            Outcome::Rebuild
        } else {
            Outcome::NotMine
        }
    }
}

/// Runs a pad action on `session` at `now`. None when `cmd` is not a pad action. On
/// success it returns the flow that ended, if one did. An error comes from the files.
pub fn act(session: &mut Session, cmd: &str, now: u64) -> Option<std::io::Result<Option<Done>>> {
    match cmd {
        "pin:submit" => Some(session.submit(now)),
        "pin:delete" => {
            session.delete();
            Some(Ok(None))
        }
        _ => {
            let digit = cmd.strip_prefix("pin:digit:")?.chars().next()?;
            session.press(digit, now);
            Some(Ok(None))
        }
    }
}

/// The pad. `back` adds a Back button, for the Settings flows. The lock has none.
/// Keys are named `pin-digit-N`, `pin-delete` and `pin-submit`.
pub fn view(services: &Services, screen: Size, session: &Session, back: bool) -> Window {
    let t = &services.theme;
    let heading = heading(session);
    let mut children = vec![if back {
        WidgetPod::new(Header::new(t, heading).back("back"))
    } else {
        title(services, heading)
    }];
    let (text, tone) = match session.notice() {
        Some(notice) => (notice_text(notice), Tone::Ink),
        None => (instruction(session), Tone::InkSecondary),
    };
    children.push(WidgetPod::new(Paragraph::new(text, t.font.body.clone(), tone)));
    children.push(
        WidgetPod::new(Label::new(
            dots(session.digit_count()),
            t.font.display.clone(),
            Tone::Ink,
        ))
        .with_key("pin-dots"),
    );
    children.push(keypad(services));
    if session.purpose() == Purpose::Unlock {
        children.push(
            WidgetPod::new(Paragraph::new(
                tr("Forgot the PIN? Connect the Kobo to a computer and delete the file .kaia/pin to remove the lock."),
                t.font.body.clone(),
                Tone::InkSecondary,
            ))
            .with_key("pin-recovery"),
        );
    }
    root_window(services, screen, page(services, children))
}

fn heading(session: &Session) -> String {
    match (session.purpose(), session.step()) {
        (_, Step::Confirm) => tr("Enter the PIN again"),
        (Purpose::Set, _) | (Purpose::Change, Step::New) => tr("Choose a PIN"),
        (Purpose::Change, _) => tr("Current PIN"),
        (Purpose::Off, _) => tr("Turn off screen lock"),
        (Purpose::Unlock, _) => tr("Screen locked"),
    }
}

fn instruction(session: &Session) -> String {
    match (session.purpose(), session.step()) {
        (_, Step::Confirm) => tr("Type the same PIN again."),
        (_, Step::New) => tr_format(
            &tr("Choose %1 to %2 digits."),
            &[&store::MIN_DIGITS.to_string(), &store::MAX_DIGITS.to_string()],
        ),
        (Purpose::Change, _) => tr("Enter the current PIN."),
        (Purpose::Off, _) => tr("Enter the PIN to turn off the lock."),
        (Purpose::Unlock, _) => tr("Enter the PIN to unlock."),
        (Purpose::Set, _) => tr("Choose a PIN."),
    }
}

fn notice_text(notice: Notice) -> String {
    match notice {
        Notice::Wrong { tries_left } => tr_format(
            &tr_n(
                "Wrong PIN. %1 try left before a wait.",
                "Wrong PIN. %1 tries left before a wait.",
                i64::from(tries_left),
            ),
            &[&tries_left.to_string()],
        ),
        Notice::Waiting { seconds } => tr_format(
            &tr_n(
                "Too many wrong PINs. Wait %1 second.",
                "Too many wrong PINs. Wait %1 seconds.",
                seconds as i64,
            ),
            &[&seconds.to_string()],
        ),
        Notice::TooShort => tr_format(
            &tr("Enter at least %1 digits."),
            &[&store::MIN_DIGITS.to_string()],
        ),
        Notice::Mismatch => tr("The PINs do not match. Choose a PIN again."),
    }
}

/// One dot per typed digit. A no-break space keeps the row its height when empty.
fn dots(count: usize) -> String {
    if count == 0 {
        return "\u{a0}".to_string();
    }
    vec!["\u{2022}"; count].join(" ")
}

fn keypad(services: &Services) -> WidgetPod {
    let t = &services.theme;
    let height = t.touch_target * 3 / 2;
    let mut rows = Vec::new();
    for digits in [[1, 2, 3], [4, 5, 6], [7, 8, 9]] {
        let keys = digits.iter().map(|&d| digit_key(services, d, height)).collect();
        rows.push(key_row(keys, t.space.md));
    }
    let bottom = vec![
        key(
            services,
            &tr("Delete"),
            t.font.body.clone(),
            "pin-delete",
            "pin:delete",
            false,
            height,
        ),
        digit_key(services, 0, height),
        key(
            services,
            &tr("OK"),
            t.font.body_bold.clone(),
            "pin-submit",
            "pin:submit",
            true,
            height,
        ),
    ];
    rows.push(key_row(bottom, t.space.md));
    // Shrink to the keys, so the text under the keypad keeps its place.
    WidgetPod::new(Column::new(rows).gap(t.space.md).shrink())
}

fn key_row(keys: Vec<WidgetPod>, gap: u32) -> WidgetPod {
    WidgetPod::new(Row::new(keys).gap(gap))
}

fn digit_key(services: &Services, digit: u8, height: u32) -> WidgetPod {
    let t = &services.theme;
    let font = FontSpec {
        size: t.font.display.size * 1.4,
        ..t.font.display.clone()
    };
    let label = digit.to_string();
    key(
        services,
        &label,
        font,
        &format!("pin-digit-{digit}"),
        &format!("pin:digit:{digit}"),
        false,
        height,
    )
}

/// A key: a bordered tile that sends `action` on a tap. The `primary` key is filled.
fn key(
    services: &Services,
    label: &str,
    font: FontSpec,
    name: &str,
    action: &str,
    primary: bool,
    height: u32,
) -> WidgetPod {
    let t = &services.theme;
    let text = WidgetPod::new(Label::new(
        label,
        font,
        if primary { Tone::Surface } else { Tone::Ink },
    ));
    let centred = WidgetPod::new(Align::new(text, Alignment::Center, Alignment::Center));
    let frame = Frame::new(centred).radius(t.radius.button);
    let frame = if primary {
        frame.fill(Tone::Ink)
    } else {
        frame.border(Tone::Ink, t.line.hairline)
    };
    let tap = WidgetPod::new(Pressable::new(WidgetPod::new(frame)).tap(action, Vec::new())).with_key(name);
    WidgetPod::new(Expanded::new(
        WidgetPod::new(SizedBox::new(Some(tap)).height(height)),
        1,
    ))
}
