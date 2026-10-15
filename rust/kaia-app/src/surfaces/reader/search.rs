//! Local search scopes and the reader's five-row keyboard.
use super::{
    chrome::{rect, Chrome},
    compact::*,
    panels::{key_of, SearchRow},
    state::{read_field, Core},
};
use crate::{
    services::Services,
    surface::{Nav, Outcome, SurfaceEnv},
    surfaces::common::window,
};
use kaia_gfx::{Point, Rect, Size};
use kaia_i18n::tr;
use kaia_ui::{Constraints, EventCx, Handled, LayoutCx, PaintCx, Tone, UiEvent, Widget, WidgetPod, Window};
use std::{cell::RefCell, rc::Rc};
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum Scope {
    #[default]
    Book,
    Annotations,
    Dictionary,
}
impl Scope {
    pub fn label(self) -> String {
        match self {
            Self::Book => tr("This book"),
            Self::Annotations => tr("Annotations"),
            Self::Dictionary => tr("Dictionary"),
        }
    }
}

pub(super) struct SearchModel<'a> {
    pub title: &'a str,
    pub text: &'a str,
    pub rows: Vec<SearchRow>,
    pub status: &'a str,
    pub history: bool,
    pub scope: Scope,
}
pub(super) fn panel(services: &Services, screen: Size, model: SearchModel<'_>) -> Window {
    let SearchModel {
        title,
        text: query,
        rows,
        status,
        history,
        scope,
    } = model;
    let mut chrome = dialog(screen);
    heading(&mut chrome, screen, &tr("Search"));
    chrome.children.pop();
    put(
        &mut chrome,
        screen,
        [45, 40, 100, 85],
        icon(screen, "chevron.left", "reader:panel:close", "panel-close"),
    );
    put(
        &mut chrome,
        screen,
        [61, 137, 1142, 86],
        WidgetPod::new(
            kaia_ui::components::Frame::new(WidgetPod::new(kaia_ui::components::SizedBox::new(None)))
                .border(Tone::Ink, 3),
        ),
    );
    put(
        &mut chrome,
        screen,
        [79, 141, 75, 78],
        icon(screen, "appbar.search", "reader:search:go", "search-go"),
    );
    let field = kaia_ui::controls::TextField::new()
        .borderless()
        .focus_on("ReaderPanelFocus")
        .with_text(query)
        .placeholder(tr("Word or phrase"))
        .on_submit("ReaderSearchSubmit");
    put(
        &mut chrome,
        screen,
        [155, 143, 720, 74],
        WidgetPod::new(field).focusable(true).with_key("search-field"),
    );
    rule(&mut chrome, screen, [879, 151, 1, 58], Tone::Ink);
    put(
        &mut chrome,
        screen,
        [881, 141, 320, 78],
        action(
            screen,
            &format!("{} ▾", scope.label()),
            "reader:search:scopes",
            "search-scope",
        ),
    );
    put(
        &mut chrome,
        screen,
        [62, 245, 1120, 80],
        action(screen, &format!("{title} ▾"), "reader:search:book", "search-book"),
    );
    put(
        &mut chrome,
        screen,
        [85, 340, 760, 70],
        text(screen, status).with_key("search-status"),
    );
    if history {
        put(
            &mut chrome,
            screen,
            [850, 340, 350, 70],
            action(
                screen,
                &tr("Clear history"),
                "reader:search:history:clear",
                "search-history-clear",
            ),
        );
    }
    let visible = !rows.is_empty() || !query.is_empty();
    let list = kaia_ui::composite::PagedList::new(rows, |row: &SearchRow, _: &kaia_ui::Theme| {
        let line = kaia_ui::composite::ListRow::new(&row.label)
            .secondary(&row.snippet)
            .on_action(&row.action);
        WidgetPod::new(line).with_key(key_of(&row.action))
    });
    put(
        &mut chrome,
        screen,
        [61, 420, 1142, 600],
        WidgetPod::new(list).visible(visible).with_key("search-results"),
    );
    put(
        &mut chrome,
        screen,
        [0, 1040, 1264, 640],
        WidgetPod::new(Keyboard::new(&services.language)).with_key("search-keyboard"),
    );
    window(services, screen, chrome)
}
pub(super) fn command(rc: &Rc<RefCell<Core>>, cmd: &str, env: &mut SurfaceEnv<'_>) -> Option<Outcome> {
    if cmd == "reader:search:book" {
        let mut c = rc.borrow_mut();
        let mut chrome = Chrome {
            panel_border: 3,
            size: env.screen,
            children: vec![],
            panels: vec![rect(env.screen, 62, 325, 700, 182)],
        };
        put(
            &mut chrome,
            env.screen,
            [66, 329, 692, 84],
            action(env.screen, &c.title, "reader:panel:close", "search-book-current"),
        );
        put(
            &mut chrome,
            env.screen,
            [66, 420, 692, 83],
            action(env.screen, &tr("My books"), "nav:library", "search-book-library"),
        );
        let window = window(env.services, env.screen, chrome);
        let serial = super::next_serial(&mut c);
        return Some(Outcome::Navigate(Nav::Push(super::panels::panel_surface(
            window,
            serial,
            rc.clone(),
        ))));
    }
    if cmd == "reader:search:scopes" {
        let mut c = rc.borrow_mut();
        if let Some(layer) = &c.panel {
            c.search_text = read_field(&layer.window, env.ctx, "search-field").unwrap_or_default();
        }
        let mut chrome = Chrome {
            panel_border: 3,
            size: env.screen,
            children: vec![],
            panels: vec![rect(env.screen, 823, 222, 380, 273)],
        };
        for (i, scope) in [Scope::Book, Scope::Annotations, Scope::Dictionary]
            .into_iter()
            .enumerate()
        {
            put(
                &mut chrome,
                env.screen,
                [827, 226 + i as u32 * 91, 372, 83],
                action(
                    env.screen,
                    &scope.label(),
                    &format!("reader:search:scope:{i}"),
                    &format!("search-scope-{i}"),
                ),
            );
        }
        for y in [313, 404] {
            rule(&mut chrome, env.screen, [827, y, 372, 1], Tone::Divider);
        }
        let window = window(env.services, env.screen, chrome);
        let serial = super::next_serial(&mut c);
        return Some(Outcome::Navigate(Nav::Push(super::panels::panel_surface(
            window,
            serial,
            rc.clone(),
        ))));
    }
    if let Some(i) = cmd
        .strip_prefix("reader:search:scope:")
        .and_then(|s| s.parse::<usize>().ok())
    {
        let mut c = rc.borrow_mut();
        c.search_scope = match i {
            1 => Scope::Annotations,
            2 => Scope::Dictionary,
            _ => Scope::Book,
        };
        let scope = c.search_scope.label();
        if let Some(layer) = &c.panel {
            layer
                .window
                .update::<super::chrome::Tool, _>(env.ctx, "search-scope", |t, cx| {
                    t.label = format!("{scope} ▾");
                    cx.request_paint();
                });
        }
        return Some(Outcome::Navigate(Nav::Pop));
    }
    if cmd != "reader:search:go" || rc.borrow().search_scope == Scope::Book {
        return None;
    }
    let mut c = rc.borrow_mut();
    let layer = c.panel.as_ref()?;
    let query = read_field(&layer.window, env.ctx, "search-field").unwrap_or_default();
    c.search_text = query.clone();
    let scope = c.search_scope;
    if query.trim().is_empty() {
        env.toast(tr("Type a word to search."), 2.0);
        return Some(Outcome::Handled);
    }
    let (rows, status) = if scope == Scope::Annotations {
        let query = query.to_lowercase();
        let rows = c
            .session
            .as_mut()
            .and_then(|s| s.annotations().ok())
            .unwrap_or_default()
            .iter()
            .filter(|a| {
                [&a.text, &a.note, &a.chapter]
                    .iter()
                    .any(|t| t.as_ref().is_some_and(|t| t.to_lowercase().contains(&query)))
            })
            .map(|a| SearchRow {
                label: a
                    .text
                    .clone()
                    .or_else(|| a.chapter.clone())
                    .unwrap_or_else(|| tr("Bookmark")),
                snippet: a.note.clone().unwrap_or_default(),
                action: format!("reader:contents:annot:{}", a.id),
            })
            .collect::<Vec<_>>();
        let status = if rows.is_empty() {
            tr("No matches.")
        } else {
            kaia_i18n::format(&tr("%1 matches."), &[&rows.len().to_string()])
        };
        (rows, status)
    } else {
        let preferred = super::preferred_dictionaries(&c);
        let answer = super::dict::lookup(query.trim(), env.services, &preferred);
        (
            vec![SearchRow {
                label: query.clone(),
                snippet: answer.text,
                action: "reader:search:dictionary".into(),
            }],
            String::new(),
        )
    };
    let window = panel(
        env.services,
        env.screen,
        SearchModel {
            title: &c.title,
            text: &query,
            rows,
            status: &status,
            history: false,
            scope,
        },
    );
    Some(super::show_panel(rc, &mut c, env.ctx, window, true))
}

#[derive(Clone)]
enum KeyAction {
    Char(char),
    Shift,
    Backspace,
    Space,
    Symbols,
    Accents,
    Left,
    Right,
    Go,
}
struct KeyBox {
    rect: Rect,
    label: String,
    action: KeyAction,
}
pub(super) struct Keyboard {
    language: String,
    shift: u8,
    mode: u8,
}
impl Keyboard {
    fn new(language: &str) -> Self {
        Self {
            language: language.into(),
            shift: 0,
            mode: 0,
        }
    }
    fn keys(&self, b: Rect) -> Vec<KeyBox> {
        let mut out = Vec::new();
        let sx = b.w as f32 / 1264.0;
        let sy = b.h as f32 / 640.0;
        let mut put = |x: f32, y: f32, w: f32, label: String, action| {
            out.push(KeyBox {
                rect: Rect::new(
                    b.x + (x * sx) as i32,
                    b.y + (y * sy) as i32,
                    (w * sx) as u32,
                    (98.0 * sy) as u32,
                ),
                label,
                action,
            })
        };
        let french = self.language.starts_with("fr");
        let german = self.language.starts_with("de");
        let rows = match self.mode {
            1 => ["1234567890", "!@#$%^&*()", "[]{}<>/\\|=", "+-_:;,.?"],
            2 => ["1234567890", "áàâäãåæçéè", "êëíìîïñóòö", "ôõøúùûüß"],
            _ => [
                "1234567890",
                if french {
                    "azertyuiop"
                } else if german {
                    "qwertzuiop"
                } else {
                    "qwertyuiop"
                },
                if french {
                    "qsdfghjklm"
                } else if self.language.starts_with("es") {
                    "asdfghjklñ"
                } else {
                    "asdfghjkl@"
                },
                if french {
                    "wxcvbn,."
                } else if german {
                    "yxcvbnm."
                } else {
                    "zxcvbnm."
                },
            ],
        };
        for (row, letters) in rows.into_iter().enumerate() {
            for (i, ch) in letters.chars().enumerate() {
                let ch = if self.shift > 0 {
                    ch.to_uppercase().next().unwrap_or(ch)
                } else {
                    ch
                };
                let index = if row == 3 { i + 1 } else { i };
                put(
                    65.0 + index as f32 * 115.0,
                    45.0 + row as f32 * 118.0,
                    98.0,
                    ch.to_string(),
                    KeyAction::Char(ch),
                );
            }
        }
        put(65.0, 399.0, 98.0, "⇧".into(), KeyAction::Shift);
        put(1100.0, 399.0, 98.0, "⌫".into(), KeyAction::Backspace);
        for (x, w, label, key) in [
            (
                65.0,
                150.0,
                if self.mode == 1 { "ABC" } else { "#&+" },
                KeyAction::Symbols,
            ),
            (
                240.0,
                150.0,
                if self.mode == 2 { "ABC" } else { "ÉÀÇ" },
                KeyAction::Accents,
            ),
            (420.0, 305.0, "␣", KeyAction::Space),
            (752.0, 110.0, "←", KeyAction::Left),
            (867.0, 110.0, "→", KeyAction::Right),
            (985.0, 214.0, "Go", KeyAction::Go),
        ] {
            put(x, 513.0, w, tr(label), key);
        }
        out
    }
}
impl Widget for Keyboard {
    kaia_ui::impl_as_any!();
    fn layout(&mut self, _: &mut LayoutCx, c: Constraints) -> Size {
        c.max
    }
    fn paint(&mut self, cx: &mut PaintCx) {
        let b = cx.bounds();
        cx.fill_rect(b, cx.color(Tone::Surface));
        cx.fill_rect(Rect::new(b.x, b.y, b.w, 2), cx.color(Tone::Ink));
        for k in self.keys(b) {
            let selected =
                matches!(k.action, KeyAction::Go) || (matches!(k.action, KeyAction::Shift) && self.shift > 0);
            if selected {
                cx.fill_rect(k.rect, cx.color(Tone::Ink));
            }
            let mut font = cx.theme.font.body.clone();
            font.size = 52.0 * b.w as f32 / 1264.0;
            let style = cx
                .theme
                .style(&font, if selected { Tone::Surface } else { Tone::Ink });
            let text = cx.fonts.layout(&k.label, &style, k.rect.w, Some(1), false);
            cx.text(
                &text,
                Point::new(
                    k.rect.x + (k.rect.w as i32 - text.width as i32) / 2,
                    k.rect.y + (k.rect.h as i32 - text.height as i32) / 2,
                ),
            );
        }
    }
    fn event(&mut self, cx: &mut EventCx, e: &UiEvent) -> Handled {
        let UiEvent::Gesture(kaia_input::Gesture::Tap { pos, .. }) = e else {
            return Handled::No;
        };
        let Some(k) = self.keys(cx.bounds()).into_iter().find(|k| k.rect.contains(*pos)) else {
            return Handled::No;
        };
        use kaia_input::Key;
        match k.action {
            KeyAction::Char(ch) => {
                cx.key_to_focus(Key::Char(ch));
                if self.shift == 1 {
                    self.shift = 0;
                }
            }
            KeyAction::Shift => self.shift = (self.shift + 1) % 3,
            KeyAction::Backspace => cx.key_to_focus(Key::Backspace),
            KeyAction::Space => cx.key_to_focus(Key::Char(' ')),
            KeyAction::Symbols => self.mode = if self.mode == 1 { 0 } else { 1 },
            KeyAction::Accents => self.mode = if self.mode == 2 { 0 } else { 2 },
            KeyAction::Left => cx.key_to_focus(Key::Left),
            KeyAction::Right => cx.key_to_focus(Key::Right),
            KeyAction::Go => cx.action(kaia_ui::AppAction {
                id: "reader:search:go".into(),
                args: vec![],
            }),
        }
        cx.request_paint();
        Handled::Yes
    }
}
