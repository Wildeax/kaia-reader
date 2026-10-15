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
                SizedBox::new(Some(WidgetPod::new(Align::new(
                    WidgetPod::new(Padding::new(
                        WidgetPod::new(Label::new(tr("More"), heading, Tone::Ink)),
                        Insets::symmetric(t.scale.dp(11), 0),
                    )),
                    Alignment::Start,
                    Alignment::Center,
                ))))
                .height(t.scale.dp(36)),
            ),
            rule(),
        ];
        for (id, label, icon) in [
            ("favorites", tr("Favorites"), "star.empty"),
            ("notes", tr("Notes"), "bookmark"),
            ("notebooks", tr("Notebooks"), "edit"),
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
                top: 0,
                bottom: 0,
            },
        );
        Some(common::root_window(
            env.services,
            env.screen,
            common::tabbed_page(
                env.services,
                Column::new(vec![WidgetPod::new(Expanded::new(WidgetPod::new(content), 1))])
                    .cross_align(CrossAlign::Stretch),
                NavItem::More,
            ),
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
            "more:notebooks" => Box::new(LocalPage::notebooks()),
            "more:experimental" => Box::new(LocalPage::experiments()),
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

/// Separate informational notebooks and the supported local experiments.
pub struct LocalPage {
    notebooks: bool,
    intro: bool,
}
impl LocalPage {
    pub fn notebooks() -> Self {
        Self {
            notebooks: true,
            intro: true,
        }
    }
    pub fn experiments() -> Self {
        Self {
            notebooks: false,
            intro: false,
        }
    }
}
impl Surface for LocalPage {
    fn route(&self) -> Route {
        if self.notebooks {
            Route::Notebooks
        } else {
            Route::Experiments
        }
    }
    fn build(&mut self, env: &mut SurfaceEnv<'_>) -> Option<Window> {
        use kaia_gfx::Rect;
        use kaia_ui::components::{Frame, Paragraph};
        use kaia_ui::controls::{Checkbox, IconButton};
        use library::{views, widgets::Overlay};
        let t = &env.services.theme;
        let title = if self.notebooks {
            tr("Notebooks")
        } else {
            tr("Experiments")
        };
        let head = WidgetPod::new(
            Row::new(vec![
                WidgetPod::new(IconButton::new("chevron.left").on_action("back")).with_key("local-back"),
                WidgetPod::new(Expanded::new(
                    WidgetPod::new(Label::new(&title, t.font.title.clone(), Tone::Ink)),
                    1,
                )),
            ])
            .cross_align(CrossAlign::Center),
        );
        let root = if self.notebooks {
            let page = Padding::new(
                WidgetPod::new(
                    Column::new(vec![
                        head,
                        rule(),
                        views::local_empty(
                            t,
                            &tr("No notebooks"),
                            &tr("Notebook creation is not available in this build."),
                            45,
                        ),
                        views::compact(t, &tr("Book notes"), "local:notes", "notebooks-notes"),
                        views::compact(t, &tr("About notebooks"), "local:intro", "notebooks-info"),
                    ])
                    .gap(t.space.md)
                    .cross_align(CrossAlign::Stretch),
                ),
                Insets::symmetric(t.scale.dp(40), 0),
            );
            let base = WidgetPod::new(common::tabbed_page(env.services, page, NavItem::More));
            if self.intro {
                let close = WidgetPod::new(Row::new(vec![
                    WidgetPod::new(Expanded::new(WidgetPod::new(SizedBox::new(None)), 1)),
                    WidgetPod::new(IconButton::new("close").on_action("local:intro-close"))
                        .with_key("notebooks-intro-close"),
                ]));
                let panel = views::sheet_with(
                    t,
                    close,
                    vec![
                        WidgetPod::new(
                            SizedBox::new(Some(WidgetPod::new(Align::new(
                                WidgetPod::new(
                                    SizedBox::new(Some(WidgetPod::new(views::EmptyBooks)))
                                        .width(t.scale.dp(155))
                                        .height(t.scale.dp(155)),
                                ),
                                Alignment::Center,
                                Alignment::Center,
                            ))))
                            .height(t.scale.dp(155)),
                        ),
                        WidgetPod::new(Paragraph::new(
                            tr("A space for writing"),
                            t.font.title.clone(),
                            Tone::Ink,
                        )),
                        WidgetPod::new(Paragraph::new(
                            tr("Freeform notebooks are not available in this build."),
                            t.font.body.clone(),
                            Tone::Ink,
                        )),
                        WidgetPod::new(Paragraph::new(
                            tr("Your highlights and book notes are available in Notes."),
                            t.font.body.clone(),
                            Tone::Ink,
                        )),
                        views::compact(t, &tr("Book notes"), "local:notes", "notebooks-intro-notes"),
                    ],
                    t.scale.dp(16),
                );
                WidgetPod::new(
                    Overlay::new(
                        Some(base),
                        panel,
                        Rect::new(116, 145, 400, 550),
                        "local:intro-close",
                    )
                    .dialog(),
                )
            } else {
                base
            }
        } else {
            let strip = |label: &str| {
                WidgetPod::new(
                    Frame::new(WidgetPod::new(Padding::new(
                        WidgetPod::new(Label::new(label, t.font.caption.clone(), Tone::Ink)),
                        Insets::symmetric(t.space.sm, t.space.xs),
                    )))
                    .fill(Tone::SurfaceAlt),
                )
            };
            let flag = |label: &str, key: &str| {
                WidgetPod::new(
                    Checkbox::new(settings::keys::bool_or(&env.services.settings, key, false))
                        .label(label)
                        .on_action(format!("local:set:{key}")),
                )
                .with_key(key)
            };
            let mut rows = vec![
                WidgetPod::new(Label::new(&title, t.font.title.clone(), Tone::Ink)),
                views::body_text(t, &tr("Optional features for this device.")),
                strip(&tr("Accessibility")),
                views::compact(
                    t,
                    &tr("Large print settings"),
                    "local:display",
                    "experiments-large-print",
                ),
                strip(&tr("Experimental features")),
                flag(&tr("Enable experiments"), settings::keys::EXPERIMENTAL_ENABLED),
            ];
            if settings::experiments::FLAGS.is_empty() {
                rows.push(views::body_text(
                    t,
                    &tr("No experimental features in this build."),
                ));
            }
            for flag_info in settings::experiments::FLAGS {
                rows.push(flag(&settings::experiments::title(flag_info.id), flag_info.key));
            }
            let panel = views::sheet_with(
                t,
                WidgetPod::new(Row::new(vec![
                    WidgetPod::new(Expanded::new(WidgetPod::new(SizedBox::new(None)), 1)),
                    WidgetPod::new(IconButton::new("close").on_action("back")).with_key("experiments-close"),
                ])),
                rows,
                t.scale.dp(20),
            );
            WidgetPod::new(Overlay::new(None, panel, Rect::new(12, 12, 608, 816), "back").dialog())
        };
        Some(common::root_window(
            env.services,
            env.screen,
            SizedBox::new(Some(root)),
        ))
    }
    fn on_command(&mut self, cmd: &str, args: &[Value], env: &mut SurfaceEnv<'_>) -> Outcome {
        match cmd {
            "local:intro" => self.intro = true,
            "local:intro-close" => self.intro = false,
            "surface:back" if self.intro => self.intro = false,
            "local:notes" => return Outcome::Navigate(Nav::Push(Box::new(notes::NotesSurface))),
            "local:display" => {
                return Outcome::Navigate(Nav::Push(Box::new(
                    settings::section::SettingsSectionSurface::new("display".into()),
                )))
            }
            _ => {
                let Some(key) = cmd.strip_prefix("local:set:") else {
                    return Outcome::NotMine;
                };
                if key != settings::keys::EXPERIMENTAL_ENABLED
                    && !settings::experiments::is_flag_key(settings::experiments::FLAGS, key)
                {
                    return Outcome::NotMine;
                }
                if let Some(on) = args.first().and_then(Value::as_bool) {
                    env.services.settings.save(key, Value::Bool(on));
                    settings::save(env);
                }
            }
        }
        Outcome::Rebuild
    }
}
