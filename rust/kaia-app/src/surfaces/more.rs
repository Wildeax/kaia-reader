//! Secondary destinations for the local library and device.

use kaia_events::Value;
use kaia_i18n::tr;
use kaia_ui::components::{
    Align, Column, CrossAlign, Divider, Expanded, Icon, Label, Padding, Row, SizedBox,
};
use kaia_ui::composite::{Dialog, DialogButton, NavItem};
use kaia_ui::geometry::{Alignment, Insets};
use kaia_ui::{Tone, WidgetPod, Window};

use crate::surface::{Nav, Outcome, Surface, SurfaceEnv};
use crate::surfaces::{common, library, notes, settings, Route};

pub struct MoreSurface;

impl Surface for MoreSurface {
    fn route(&self) -> Route {
        Route::More
    }

    fn build(&mut self, env: &mut SurfaceEnv<'_>) -> Option<Window> {
        let t = &env.services.theme;
        let mut heading = t.font.display.clone();
        heading.size = t.scale.dp(26) as f32 * t.text_scale;
        heading.weight = 400;
        let mut children = vec![
            WidgetPod::new(
                SizedBox::new(Some(WidgetPod::new(Label::new(
                    library::data::status_line(env.services).unwrap_or_default(),
                    t.font.caption.clone(),
                    Tone::InkSecondary,
                ))))
                .height(t.scale.dp(20)),
            ),
            WidgetPod::new(
                SizedBox::new(Some(WidgetPod::new(Align::new(
                    WidgetPod::new(Padding::new(
                        WidgetPod::new(Label::new(tr("More"), heading, Tone::Ink)),
                        Insets::symmetric(t.scale.dp(11), 0),
                    )),
                    Alignment::Start,
                    Alignment::Center,
                ))))
                .height(t.scale.dp(58)),
            ),
            rule(),
        ];
        for (id, label, icon) in [
            ("favorites", tr("Favorites"), "star.empty"),
            ("notes", tr("Notes"), "bookmark"),
            ("statistics", tr("Activity"), "book.opened"),
            ("experimental", tr("Experiments"), "appbar.tools"),
            ("settings", tr("Settings"), "appbar.settings"),
            ("help", tr("Help"), "notice-question"),
        ] {
            let mut font = t.font.book_title.clone();
            font.size = t.scale.dp(17) as f32 * t.text_scale;
            font.weight = 400;
            let row = Row::new(vec![
                WidgetPod::new(Icon::new(icon, t.scale.dp(24), Tone::Ink)),
                WidgetPod::new(Expanded::new(
                    WidgetPod::new(Align::new(
                        WidgetPod::new(Label::new(label, font, Tone::Ink)),
                        Alignment::Start,
                        Alignment::Center,
                    )),
                    1,
                )),
            ])
            .gap(t.scale.dp(20))
            .cross_align(CrossAlign::Center);
            let row = Padding::new(WidgetPod::new(row), Insets::symmetric(t.scale.dp(9), 0));
            let row = SizedBox::new(Some(WidgetPod::new(row))).height(t.scale.dp(67) - 1);
            children.push(
                WidgetPod::new(
                    library::press::Pressable::new(WidgetPod::new(row)).tap(format!("more:{id}"), Vec::new()),
                )
                .with_key(format!("more-{id}"))
                .focusable(true),
            );
            children.push(rule());
        }
        let content = Padding::new(
            WidgetPod::new(Column::new(children).cross_align(CrossAlign::Stretch)),
            Insets {
                left: t.scale.dp(33),
                right: t.scale.dp(33),
                top: t.scale.dp(32),
                bottom: 0,
            },
        );
        Some(common::root_window(
            env.services,
            env.screen,
            common::tabbed_page(content, NavItem::More),
        ))
    }

    fn on_command(&mut self, cmd: &str, _args: &[Value], env: &mut SurfaceEnv<'_>) -> Outcome {
        let next: Box<dyn Surface> = match cmd {
            "more:favorites" => Box::new(library::LibrarySurface::favorites()),
            "more:notes" => Box::new(notes::NotesSurface),
            "more:settings" => Box::new(settings::SettingsSurface),
            "more:statistics" => Box::new(settings::section::SettingsSectionSurface::new(
                "statistics".into(),
            )),
            "more:experimental" => Box::new(settings::section::SettingsSectionSurface::new(
                "experimental".into(),
            )),
            "more:help" => {
                env.dialog(Dialog::new(
                    env.theme(),
                    tr("Help"),
                    tr("Copy books into the Books folder over USB. Open My books to choose a book. Select text while reading to add a note. Find your book notes and export them from Notes."),
                    vec![DialogButton::new(tr("Close"), "dialog:dismiss")],
                ));
                return Outcome::Handled;
            }
            _ => return Outcome::NotMine,
        };
        Outcome::Navigate(Nav::Push(next))
    }
}

fn rule() -> WidgetPod {
    WidgetPod::new(SizedBox::new(Some(WidgetPod::new(Divider::new(Tone::Ink)))).height(1))
}
