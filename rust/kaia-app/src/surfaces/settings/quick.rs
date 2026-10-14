//! The quick settings sheet: frontlight and warmth sliders, a Wi-Fi switch, a
//! fixed orientation message and a Sleep button. The app opens it from a swipe down
//! with `env.sheet(quick_settings(env.services))`.
//!
//! The sheet's controls send `settings:quick:<name>` ids. The top surface does
//! not own them, so the app passes them to [`command`] before global routing
//! (see the report for the exact change). The Sleep button sends the global id
//! `sleep`, which the app handles.

use crate::slider::Slider;
use kaia_events::Value;
use kaia_i18n::{format as tr_format, tr};
use kaia_ui::components::{Column, CrossAlign, Expanded, Label, Row};
use kaia_ui::composite::{ListRow, SheetPanel};
use kaia_ui::controls::{Button, ButtonStyle};
use kaia_ui::{Theme, Tone, WidgetPod};

use super::{commands, sections};
use crate::services::Services;
use crate::surface::{Outcome, SurfaceEnv};

/// The quick settings sheet, with the values the platform and settings hold now.
pub fn quick_settings(services: &Services) -> SheetPanel {
    let t = &services.theme;
    let kobo = sections::has_frontlight(services);
    let frontlight = f64::from(services.platform.frontlight.intensity());
    let warmth = f64::from(services.platform.frontlight.warmth());
    let wifi_on = services.platform.network.is_wifi_on();

    let mut rows = Vec::new();
    rows.push(slider(
        &tr("Frontlight"),
        frontlight,
        kobo,
        "settings:quick:frontlight",
        "quick-frontlight",
        t,
    ));
    rows.push(slider(
        &tr("Warmth"),
        warmth,
        kobo,
        "settings:quick:warmth",
        "quick-warmth",
        t,
    ));
    if !kobo {
        rows.push(WidgetPod::new(Label::new(
            tr("The light controls work on the Kobo only."),
            t.font.caption.clone(),
            Tone::InkSecondary,
        )));
    }
    rows.push(
        WidgetPod::new(
            ListRow::new(tr("Wi-Fi"))
                .toggle(wifi_on)
                .on_action("settings:quick:wifi"),
        )
        .with_key("quick-wifi"),
    );
    rows.push(
        WidgetPod::new(
            ListRow::new(tr("Fixed orientation")).secondary(tr("Automatic rotation is not available yet.")),
        )
        .with_key("quick-rotation"),
    );
    rows.push(WidgetPod::new(
        Row::new(vec![
            WidgetPod::new(Expanded::new(
                WidgetPod::new(
                    Button::new(ButtonStyle::Secondary)
                        .text(tr("Toggle light"))
                        .on_action("settings:quick:toggle-light"),
                )
                .with_key("quick-toggle-light"),
                1,
            )),
            WidgetPod::new(Expanded::new(
                WidgetPod::new(
                    Button::new(ButtonStyle::Primary)
                        .text(tr("Sleep"))
                        .on_action("sleep"),
                )
                .with_key("quick-sleep"),
                1,
            )),
        ])
        .gap(t.space.md),
    ));

    // The sheet is short, so its rows are spaced by the small gap.
    let content = WidgetPod::new(Column::new(rows).gap(t.space.sm).cross_align(CrossAlign::Stretch));
    SheetPanel::new(t, tr("Quick settings"), content)
}

fn slider(label: &str, value: f64, enabled: bool, id: &str, key: &str, t: &Theme) -> WidgetPod {
    let control = Slider::new(value, 0.0, 100.0, 1.0)
        .format(|v| tr_format(&tr("%1%"), &[&v.to_string()]))
        .enabled(enabled)
        .on_action(id);
    WidgetPod::new(
        Column::new(vec![
            WidgetPod::new(Label::new(label, t.font.body.clone(), Tone::Ink)),
            WidgetPod::new(control),
        ])
        .gap(t.space.sm)
        .cross_align(CrossAlign::Stretch)
        .shrink(),
    )
    .with_key(key)
}

/// Runs a control of the quick settings sheet. Returns NotMine for other ids.
pub fn command(cmd: &str, args: &[Value], env: &mut SurfaceEnv<'_>) -> Outcome {
    let Some(commands::Command::Quick(name)) = commands::parse(cmd) else {
        return Outcome::NotMine;
    };
    match name.as_str() {
        "frontlight:release" => Outcome::Handled,
        "warmth:release" => {
            super::save(env);
            Outcome::Handled
        }
        "frontlight" | "warmth" => {
            let Some(v) = args.first().and_then(Value::as_f64) else {
                return Outcome::NotMine;
            };
            let v = v.round().clamp(0.0, 100.0) as u8;
            let result = if name == "frontlight" {
                env.services.platform.frontlight.set_intensity(v)
            } else {
                env.services.platform.frontlight.set_warmth(v)
            };
            if let Err(e) = result {
                env.notice(tr_format(&tr("cannot set the light: %1"), &[&e.to_string()]));
            }
            if name == "warmth" {
                env.services
                    .settings
                    .save("display.warmth", Value::Number(f64::from(v)));
                if args.get(1).and_then(Value::as_bool) != Some(true) {
                    super::save(env);
                }
            }
            Outcome::Handled
        }
        "rotation" => {
            env.toast(tr("Automatic rotation is not available yet."), 3.0);
            Outcome::Handled
        }
        _ => Outcome::NotMine,
    }
}
