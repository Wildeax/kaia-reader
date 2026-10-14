//! The rows of a settings section. A section builds a list of [`Entry`] values
//! from the settings and the device; [`widget`] turns each one into a widget.
//! The list is paged, so the entries are plain data and cheap to build again.

use kaia_i18n::{format as tr_format, tr, tr_n};
use kaia_ui::components::{
    Column, CrossAlign, Expanded, Frame, Label, Padding, Paragraph, Row, SizedBox, Spacer,
};
use kaia_ui::composite::ListRow;
use kaia_ui::controls::{Button, ButtonStyle, RadioGroup, Slider, Stepper, TextField};
use kaia_ui::geometry::Insets;
use kaia_ui::{Theme, Tone, WidgetPod};

use super::commands::Light;
use crate::surfaces::common::button;

/// One line of a section.
#[derive(Clone, Debug, PartialEq)]
pub enum Entry {
    DeviceToggle {
        key: &'static str,
        label: String,
        on: bool,
        enabled: bool,
    },
    /// A small heading above a group of rows.
    Heading(String),
    /// A short explanation. Wraps to as many lines as it needs.
    Text(String),
    /// A switch. Its action writes `settings:set:<key>` with the new state.
    Toggle {
        key: &'static str,
        label: String,
        on: bool,
    },
    /// A row with a checkbox, for one choice of a long list such as the languages.
    /// Its action sends `action`.
    Choice {
        label: String,
        selected: bool,
        action: String,
    },
    /// A plugin switch with a discovered name.
    PluginToggle { key: String, label: String, on: bool },
    /// A stepper. Its action writes `settings:set:<key>` with the new value.
    Stepper {
        key: &'static str,
        label: String,
        value: f64,
        min: f64,
        max: f64,
        step: f64,
        unit: StepUnit,
    },
    /// A radio group. Its action writes `settings:pick:<key>` with the chosen index.
    Radio {
        key: &'static str,
        options: Vec<String>,
        selected: usize,
    },
    /// A slider for a platform value. Its action writes `settings:light:<which>`.
    Slider {
        which: Light,
        label: String,
        value: f64,
        enabled: bool,
    },
    /// A text field. Each change writes `settings:set:<key>` with the text.
    Field {
        key: &'static str,
        label: String,
        value: String,
        password: bool,
    },
    /// A button with an action id.
    Button {
        key: &'static str,
        label: String,
        action: String,
    },
    /// A list row: primary text, an optional line below it, an optional value at the
    /// end, and an optional action. Without an action the row shows its text only.
    Row {
        primary: String,
        secondary: Option<String>,
        value: Option<String>,
        action: Option<String>,
    },
    /// A bar for one day or one book: the fraction of the longest bar, and the text.
    Bar {
        label: String,
        detail: String,
        fraction: f64,
    },
    /// One installed dictionary. It shows its name with an on or off switch, and move
    /// up and move down buttons. `index` is its place in the lookup order. The switch
    /// sends `settings:dicts:toggle:<index>`, and the buttons send
    /// `settings:dicts:up:<index>` and `settings:dicts:down:<index>`.
    Dictionary {
        index: usize,
        name: String,
        enabled: bool,
    },
}

/// The unit that follows a stepper value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StepUnit {
    KoreaderUnits,
    Pages,
    /// Zero reads as `Chapter`. Any other value reads as a page count.
    ChapterOrPages,
    Minutes,
    ClockTime,
    Percent,
}

/// The widest bar, in multiples of the row height.
const BAR_ROW_HEIGHTS: u32 = 4;
/// The label column of a bar, in multiples of the row height.
const BAR_LABEL_ROW_HEIGHTS: u32 = 2;

/// The widget for one entry of a section.
pub fn widget(entry: &Entry, t: &Theme) -> WidgetPod {
    match entry {
        Entry::DeviceToggle {
            key,
            label,
            on,
            enabled,
        } => WidgetPod::new(
            ListRow::new(label.clone())
                .toggle(*on)
                .enabled(*enabled)
                .on_action(format!("settings:set:{key}")),
        )
        .with_key(format!("set-{key}")),
        // A heading starts a new group, so it has a space above it.
        Entry::Heading(text) => WidgetPod::new(Padding::new(
            WidgetPod::new(Label::new(text.clone(), t.font.label.clone(), Tone::InkSecondary)),
            Insets {
                left: 0,
                top: t.space.md,
                right: 0,
                bottom: 0,
            },
        )),
        Entry::Text(text) => WidgetPod::new(Paragraph::new(
            text.clone(),
            t.font.caption.clone(),
            Tone::InkSecondary,
        )),
        Entry::Toggle { key, label, on } => WidgetPod::new(
            ListRow::new(label.clone())
                .toggle(*on)
                .on_action(format!("settings:set:{key}")),
        )
        .with_key(format!("set-{key}")),
        Entry::Choice {
            label,
            selected,
            action,
        } => WidgetPod::new(
            ListRow::new(label.clone())
                .checkbox(*selected)
                .on_action(action.clone()),
        )
        .with_key(action.clone()),
        Entry::PluginToggle { key, label, on } => WidgetPod::new(
            ListRow::new(label.clone())
                .toggle(*on)
                .on_action(format!("settings:set:{key}")),
        )
        .with_key(format!("set-{key}")),
        Entry::Stepper {
            key,
            label,
            value,
            min,
            max,
            step,
            unit,
        } => {
            let unit = *unit;
            let stepper = Stepper::new(*value, *min, *max, *step)
                .format(move |v| unit_text(unit, v))
                .on_action(format!("settings:set:{key}"));
            WidgetPod::new(
                Row::new(vec![
                    WidgetPod::new(Expanded::new(
                        WidgetPod::new(Label::new(label.clone(), t.font.body.clone(), Tone::Ink)),
                        1,
                    )),
                    WidgetPod::new(stepper).with_key(format!("set-{key}")),
                ])
                .gap(t.space.md)
                .cross_align(CrossAlign::Center),
            )
        }
        Entry::Radio {
            key,
            options,
            selected,
        } => WidgetPod::new(
            RadioGroup::new(options.clone(), Some(*selected)).on_action(format!("settings:pick:{key}")),
        )
        .with_key(format!("pick-{key}")),
        Entry::Slider {
            which,
            label,
            value,
            enabled,
        } => {
            let id = match which {
                Light::Frontlight => "settings:light:frontlight",
                Light::Warmth => "settings:light:warmth",
            };
            let slider = Slider::new(*value, 0.0, 100.0, 1.0)
                .format(|v| tr_format(&tr("%1%"), &[&v.to_string()]))
                .enabled(*enabled)
                .on_action(id);
            WidgetPod::new(
                Column::new(vec![
                    WidgetPod::new(Label::new(label.clone(), t.font.body.clone(), Tone::Ink)),
                    WidgetPod::new(slider),
                ])
                .gap(t.space.sm)
                .cross_align(CrossAlign::Stretch)
                .shrink(),
            )
            .with_key(format!("light-{}", id.trim_start_matches("settings:light:")))
        }
        Entry::Field {
            key,
            label,
            value,
            password,
        } => {
            let field = TextField::new()
                .with_text(value.clone())
                .password(*password)
                .on_change(format!("settings:set:{key}"));
            WidgetPod::new(
                Column::new(vec![
                    WidgetPod::new(Label::new(
                        label.clone(),
                        t.font.label.clone(),
                        Tone::InkSecondary,
                    )),
                    WidgetPod::new(field)
                        .focusable(true)
                        .with_key(format!("text-{key}")),
                ])
                .gap(t.space.xs)
                .cross_align(CrossAlign::Stretch)
                .shrink(),
            )
            .with_key(format!("field-{key}"))
        }
        Entry::Button { key, label, action } => WidgetPod::new(
            Button::new(ButtonStyle::Secondary)
                .text(label.clone())
                .on_action(action.clone()),
        )
        .with_key(*key),
        Entry::Row {
            primary,
            secondary,
            value,
            action,
        } => {
            let mut row = ListRow::new(primary.clone());
            if let Some(text) = secondary {
                row = row.secondary(text.clone());
            }
            if let Some(text) = value {
                row = row.value(text.clone());
            }
            if let Some(action) = action {
                row = row.chevron().on_action(action.clone());
            }
            WidgetPod::new(row).with_key(action.clone().unwrap_or_else(|| primary.clone()))
        }
        Entry::Bar {
            label,
            detail,
            fraction,
        } => bar(label, detail, *fraction, t),
        Entry::Dictionary { index, name, enabled } => {
            let switch = WidgetPod::new(
                ListRow::new(name.clone())
                    .toggle(*enabled)
                    .on_action(format!("settings:dicts:toggle:{index}")),
            )
            .with_key(format!("dict-toggle-{index}"));
            let up = WidgetPod::new(Expanded::new(
                button(
                    ButtonStyle::Secondary,
                    &tr("Move up"),
                    &format!("settings:dicts:up:{index}"),
                    &format!("dict-up-{index}"),
                ),
                1,
            ));
            let down = WidgetPod::new(Expanded::new(
                button(
                    ButtonStyle::Secondary,
                    &tr("Move down"),
                    &format!("settings:dicts:down:{index}"),
                    &format!("dict-down-{index}"),
                ),
                1,
            ));
            WidgetPod::new(
                Column::new(vec![
                    switch,
                    WidgetPod::new(Row::new(vec![up, down]).gap(t.space.sm)),
                ])
                .gap(t.space.sm)
                .cross_align(CrossAlign::Stretch)
                .shrink(),
            )
        }
    }
}

/// A stepper value with its unit, such as `22 KOReader units` or `1 page`.
fn unit_text(unit: StepUnit, v: f64) -> String {
    let n = v.to_string();
    match unit {
        StepUnit::KoreaderUnits => tr_format(&tr("%1 KOReader units"), &[&n]),
        StepUnit::Pages => tr_format(&tr_n("%1 page", "%1 pages", v as i64), &[&n]),
        StepUnit::ChapterOrPages if v < 1.0 => tr("Chapter"),
        StepUnit::ChapterOrPages => tr_format(&tr_n("%1 page", "%1 pages", v as i64), &[&n]),
        StepUnit::Minutes => tr_format(&tr("%1 min"), &[&n]),
        StepUnit::ClockTime => {
            let minutes = (v.round() as u32) % 1440;
            tr_format(
                &tr("%1:%2"),
                &[&format!("{:02}", minutes / 60), &format!("{:02}", minutes % 60)],
            )
        }
        StepUnit::Percent => tr_format(&tr("%1%"), &[&n]),
    }
}

/// One day or one book: the label, a bar scaled to the longest one, and the
/// detail text. Every bar starts in the same column, and every detail ends at the
/// right edge, so the rows line up. A zero bar draws nothing.
fn bar(label: &str, detail: &str, fraction: f64, t: &Theme) -> WidgetPod {
    let label_w = t.row_height * BAR_LABEL_ROW_HEIGHTS;
    let max_w = t.row_height * BAR_ROW_HEIGHTS;
    let w = (fraction.clamp(0.0, 1.0) * f64::from(max_w)).round() as u32;
    let fill = (w > 0).then(|| WidgetPod::new(Frame::new(WidgetPod::new(Spacer::new(1))).fill(Tone::Accent)));
    let bar_area = WidgetPod::new(
        Row::new(vec![WidgetPod::new(
            SizedBox::new(fill).width(w).height(t.space.md),
        )])
        .cross_align(CrossAlign::Center),
    );
    let children = vec![
        WidgetPod::new(
            SizedBox::new(Some(WidgetPod::new(Label::new(
                label,
                t.font.body.clone(),
                Tone::Ink,
            ))))
            .width(label_w),
        ),
        WidgetPod::new(Expanded::new(bar_area, 1)),
        WidgetPod::new(Label::new(detail, t.font.body.clone(), Tone::InkSecondary)),
    ];
    WidgetPod::new(Row::new(children).gap(t.space.sm).cross_align(CrossAlign::Center))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entries_compare_by_value() {
        let a = Entry::Heading("Text".into());
        assert_eq!(a.clone(), Entry::Heading("Text".into()));
    }

    #[test]
    fn natural_light_stepper_values_use_clock_time_and_percent() {
        assert_eq!(unit_text(StepUnit::ClockTime, 1380.0), "23:00");
        assert_eq!(unit_text(StepUnit::ClockTime, 30.0), "00:30");
        assert_eq!(unit_text(StepUnit::Percent, 92.0), "92%");
    }
}
