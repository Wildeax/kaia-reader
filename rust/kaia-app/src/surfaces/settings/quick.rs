//! The quick settings sheet: frontlight and warmth sliders, a Wi-Fi switch, a
//! rotation lock switch and a Sleep button. The app opens it from a swipe down
//! with `env.sheet(quick_settings(env.services))`.
//!
//! The sheet's controls send `settings:quick:<name>` ids. The top surface does
//! not own them, so the app passes them to [`command`] before global routing
//! (see the report for the exact change). The Sleep button sends the global id
//! `sleep`, which the app handles.

use kaia_events::Value;
use kaia_ui::components::{Column, CrossAlign, Expanded, Label, Row};
use kaia_ui::composite::{ListRow, SheetPanel};
use kaia_ui::controls::{Button, ButtonStyle, Slider};
use kaia_ui::{Theme, Tone, WidgetPod};

use super::{commands, keys, sections};
use crate::services::Services;
use crate::surface::{Outcome, SurfaceEnv};

/// The quick settings sheet, with the values the platform and settings hold now.
pub fn quick_settings(services: &Services) -> SheetPanel {
    let t = &services.theme;
    let kobo = sections::has_frontlight(services);
    let frontlight = f64::from(services.platform.frontlight.intensity());
    let warmth = f64::from(services.platform.frontlight.warmth());
    let wifi_on = services.platform.network.is_wifi_on();
    let rotation = keys::bool_or(&services.settings, keys::DISPLAY_ROTATION_LOCK, false);

    let mut rows = Vec::new();
    rows.push(slider(
        "Frontlight",
        frontlight,
        kobo,
        "settings:quick:frontlight",
        "quick-frontlight",
        t,
    ));
    rows.push(slider(
        "Warmth",
        warmth,
        kobo,
        "settings:quick:warmth",
        "quick-warmth",
        t,
    ));
    if !kobo {
        rows.push(WidgetPod::new(Label::new(
            "The light controls work on the Kobo only.",
            t.font.caption.clone(),
            Tone::InkSecondary,
        )));
    }
    rows.push(
        WidgetPod::new(
            ListRow::new("Wi-Fi")
                .toggle(wifi_on)
                .on_action("settings:quick:wifi"),
        )
        .with_key("quick-wifi"),
    );
    rows.push(
        WidgetPod::new(
            ListRow::new("Lock rotation")
                .toggle(rotation)
                .on_action("settings:quick:rotation"),
        )
        .with_key("quick-rotation"),
    );
    rows.push(WidgetPod::new(
        Row::new(vec![
            WidgetPod::new(Expanded::new(
                WidgetPod::new(
                    Button::new(ButtonStyle::Secondary)
                        .text("Toggle light")
                        .on_action("settings:quick:toggle-light"),
                )
                .with_key("quick-toggle-light"),
                1,
            )),
            WidgetPod::new(Expanded::new(
                WidgetPod::new(Button::new(ButtonStyle::Primary).text("Sleep").on_action("sleep"))
                    .with_key("quick-sleep"),
                1,
            )),
        ])
        .gap(t.space.md),
    ));

    // The sheet is short, so its rows are spaced by the small gap.
    let content = WidgetPod::new(Column::new(rows).gap(t.space.sm).cross_align(CrossAlign::Stretch));
    SheetPanel::new(t, "Quick settings", content)
}

fn slider(label: &str, value: f64, enabled: bool, id: &str, key: &str, t: &Theme) -> WidgetPod {
    let control = Slider::new(value, 0.0, 100.0, 1.0)
        .format(|v| format!("{v}%"))
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
                env.notice(format!("cannot set the light: {e}"));
            }
            if name == "warmth" {
                env.services
                    .settings
                    .save("display.warmth", Value::Number(f64::from(v)));
                super::save(env);
            }
            Outcome::Handled
        }
        "wifi" => {
            let Some(on) = args.first().and_then(Value::as_bool) else {
                return Outcome::NotMine;
            };
            let result = if on {
                env.services.platform.network.enable_wifi()
            } else {
                env.services.platform.network.disable_wifi()
            };
            if let Err(e) = result {
                env.toast(format!("Wi-Fi did not change: {e}"), 4.0);
            }
            Outcome::Handled
        }
        "rotation" => {
            let Some(on) = args.first().and_then(Value::as_bool) else {
                return Outcome::NotMine;
            };
            env.services
                .settings
                .save(keys::DISPLAY_ROTATION_LOCK, Value::Bool(on));
            super::save(env);
            Outcome::Handled
        }
        _ => Outcome::NotMine,
    }
}
