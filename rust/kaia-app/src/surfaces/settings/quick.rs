//! The quick settings sheet: frontlight and warmth sliders, a Wi-Fi switch, an
//! airplane mode switch, a manual orientation picker and a Sleep button. The app
//! opens it from a swipe down with `env.sheet(quick_settings(env.services))`.
//!
//! The sheet's controls send `settings:quick:<name>` ids. The top surface does
//! not own them, so the app passes them to [`command`] before global routing
//! in the app loop. The Sleep button sends the global id
//! `sleep`, which the app handles.

use crate::slider::Slider;
use kaia_events::Value;
use kaia_i18n::{format as tr_format, tr};
use kaia_ui::components::{Column, CrossAlign, Expanded, Label, Row};
use kaia_ui::composite::{ListRow, SheetPanel};
use kaia_ui::controls::{Button, ButtonStyle};
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
    let airplane = keys::airplane_mode(&services.settings);

    let sliders = vec![
        slider(
            &tr("Frontlight"),
            frontlight,
            kobo,
            "settings:quick:frontlight",
            "quick-frontlight",
            t,
        ),
        slider(
            &tr("Warmth"),
            warmth,
            kobo,
            "settings:quick:warmth",
            "quick-warmth",
            t,
        ),
    ];
    let mut rows = if t.scale.width > t.scale.height {
        vec![equal_row(sliders, t)]
    } else {
        sliders
    };
    if !kobo {
        rows.push(WidgetPod::new(Label::new(
            tr("The light controls work on the Kobo only."),
            t.font.caption.clone(),
            Tone::InkSecondary,
        )));
    }
    // While airplane mode is on, the Wi-Fi switch only shows a notice.
    let wifi_action = if airplane {
        "settings:quick:wifi-blocked"
    } else {
        "settings:quick:wifi"
    };
    rows.push(equal_row(
        vec![
            WidgetPod::new(ListRow::new(tr("Wi-Fi")).toggle(wifi_on).on_action(wifi_action))
                .with_key("quick-wifi"),
            WidgetPod::new(
                ListRow::new(tr("Airplane mode"))
                    .toggle(airplane)
                    .on_action("settings:quick:airplane"),
            )
            .with_key("quick-airplane"),
        ],
        t,
    ));
    rows.push(equal_row(
        vec![WidgetPod::new(
            ListRow::new(tr("Lock orientation"))
                .toggle(keys::bool_or(
                    &services.settings,
                    keys::DISPLAY_ROTATION_LOCK,
                    false,
                ))
                .on_action("settings:quick:rotation-lock"),
        )
        .with_key("quick-rotation")],
        t,
    ));
    rows.push(WidgetPod::new(
        Row::new(vec![
            WidgetPod::new(Expanded::new(
                WidgetPod::new(
                    Button::new(ButtonStyle::Secondary)
                        .text(tr("Orientation"))
                        .on_action("settings:quick:orientation"),
                )
                .with_key("quick-orientation"),
                1,
            )),
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

fn equal_row(children: Vec<WidgetPod>, t: &Theme) -> WidgetPod {
    WidgetPod::new(
        Row::new(
            children
                .into_iter()
                .map(|child| WidgetPod::new(Expanded::new(child, 1)))
                .collect(),
        )
        .gap(t.space.md),
    )
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
        "wifi-blocked" => {
            env.notice(tr("Airplane mode is on. Turn it off to use Wi-Fi."));
            Outcome::Handled
        }
        "airplane" => {
            let Some(on) = args.first().and_then(Value::as_bool) else {
                return Outcome::NotMine;
            };
            env.services
                .settings
                .save(keys::NETWORK_AIRPLANE_MODE, Value::Bool(on));
            super::save(env);
            // `App::command` turns Wi-Fi off when airplane mode goes on, on the script
            // worker like the Wi-Fi switch. It stays off until the switch is turned
            // off and the Wi-Fi switch is used again.
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
            let key = if name == "warmth" {
                "display.warmth"
            } else {
                "display.frontlight"
            };
            env.services.settings.save(key, Value::Number(f64::from(v)));
            if args.get(1).and_then(Value::as_bool) != Some(true) {
                super::save(env);
            }
            Outcome::Handled
        }
        _ => Outcome::NotMine,
    }
}

/// The four manual orientations, in clockwise order.
pub fn orientation_labels() -> Vec<String> {
    vec![
        tr("Portrait"),
        tr("Landscape"),
        tr("Portrait, upside down"),
        tr("Landscape, upside down"),
    ]
}

pub fn orientation_picker(services: &Services) -> SheetPanel {
    let degrees = keys::number_or(
        &services.settings,
        keys::DISPLAY_ROTATION,
        f64::from(services.platform.device.startup_rotation_degrees()),
    );
    let choices = kaia_ui::controls::RadioGroup::new(orientation_labels(), Some((degrees / 90.0) as usize))
        .on_action("settings:quick:orientation-pick");
    SheetPanel::new(&services.theme, tr("Orientation"), WidgetPod::new(choices))
}
