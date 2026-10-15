//! General device controls, separate from the anchored light panel.
//! Commands also handle the light panel's live values and persistence.

use super::widgets::CompactRow as ListRow;
use kaia_events::Value;
use kaia_i18n::{format as tr_format, tr};
use kaia_ui::components::{Column, CrossAlign, Expanded, Row};
use kaia_ui::composite::SheetPanel;
use kaia_ui::{Theme, WidgetPod};

use super::{commands, keys};
use crate::services::Services;
use crate::surface::{Outcome, SurfaceEnv};

/// The quick settings sheet, with the values the platform and settings hold now.
pub fn quick_settings(services: &Services) -> SheetPanel {
    let t = &services.theme;
    let wifi_on = services.platform.network.is_wifi_on();
    let airplane = keys::airplane_mode(&services.settings);
    let mut rows = vec![equal_row(
        vec![
            WidgetPod::new(ListRow::new(tr("Light")).on_action("settings:light:open"))
                .with_key("quick-light-panel"),
            WidgetPod::new(ListRow::new(tr("Wi-Fi settings")).on_action("settings:wifi:open"))
                .with_key("quick-wifi-settings"),
        ],
        t,
    )];
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
    rows.push(equal_row(
        vec![
            WidgetPod::new(ListRow::new(tr("Orientation")).on_action("settings:quick:orientation"))
                .with_key("quick-orientation"),
            WidgetPod::new(ListRow::new(tr("Toggle light")).on_action("settings:quick:toggle-light"))
                .with_key("quick-toggle-light"),
            WidgetPod::new(ListRow::new(tr("Sleep")).on_action("sleep")).with_key("quick-sleep"),
        ],
        t,
    ));

    // The sheet is short, so its rows are spaced by the small gap.
    let content = WidgetPod::new(
        Column::new(rows)
            .gap(t.space.sm)
            .cross_align(CrossAlign::Stretch)
            .shrink(),
    );
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

/// Runs a control of the quick settings sheet. Returns NotMine for other ids.
pub fn command(cmd: &str, args: &[Value], env: &mut SurfaceEnv<'_>) -> Outcome {
    let Some(commands::Command::Quick(name)) = commands::parse(cmd) else {
        return Outcome::NotMine;
    };
    match name.as_str() {
        "auto" | "bedtime" => {
            let (key, value) = if name == "auto" {
                let Some(on) = args.first().and_then(Value::as_bool) else {
                    return Outcome::Handled;
                };
                (keys::DISPLAY_NATURAL_LIGHT_ENABLED, Value::Bool(on))
            } else {
                let Some(minutes) = args.first().and_then(Value::as_f64).filter(|v| v.is_finite()) else {
                    return Outcome::Handled;
                };
                (
                    keys::DISPLAY_NATURAL_LIGHT_BEDTIME,
                    Value::Number(minutes.round().clamp(0.0, 1439.0)),
                )
            };
            env.services.settings.save(key, value);
            super::save(env);
            Outcome::Handled
        }
        "frontlight:release" | "warmth:release" => {
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
