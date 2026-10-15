//! Reader typography and a draft that is committed only by Apply.
use super::{
    chrome::{rect, Chrome, Tool},
    compact::*,
    panels,
    state::{Core, Layer},
};
use crate::{
    services::Services,
    surface::{Nav, Outcome, SurfaceEnv},
    surfaces::common::window,
};
use kaia_book::TextLayout;
use kaia_events::Value;
use kaia_gfx::{Point, Size};
use kaia_i18n::tr;
use kaia_ui::{Tone, WidgetPod, Window};
use std::{cell::RefCell, rc::Rc};

pub(super) struct Advanced {
    pub before: TextLayout,
    pub draft: TextLayout,
    pub layer: Layer,
}

pub(super) fn panel(
    services: &Services,
    screen: Size,
    layout: &TextLayout,
    model: &super::overlay::OverlayModel,
) -> Window {
    let mut chrome = super::overlay::chrome(services, screen, model, Some("overlay-layout"));
    chrome.panels.push(rect(screen, 0, 179, 1264, 562));
    controls(&mut chrome, services, screen, layout);
    window(services, screen, chrome)
}
fn ranges(screen: Size) -> [[f64; 3]; 3] {
    [
        [
            super::koreader_px(screen, 12.0) as f64,
            super::koreader_px(screen, 72.0) as f64,
            1.0,
        ],
        [0.8, 2.3, 0.05],
        [0.0, super::koreader_px(screen, 88.0) as f64, 1.0],
    ]
}
fn controls(chrome: &mut Chrome, _services: &Services, screen: Size, l: &TextLayout) {
    put(chrome, screen, [85, 259, 250, 60], text(screen, &tr("Font")));
    let face = panels::font_choice_label(panels::font_index(l.font_family.as_deref()));
    put(
        chrome,
        screen,
        [338, 239, 573, 80],
        dropdown(screen, &face, "reader:layout:fonts", "layout-font"),
    );
    let mut advanced = Tool::new("", "reader:advanced:open", 48 * screen.w.min(screen.h) / 1264);
    advanced.label = tr("Advanced");
    advanced.bold = true;
    put(
        chrome,
        screen,
        [938, 239, 290, 80],
        WidgetPod::new(advanced)
            .focusable(true)
            .with_key("layout-advanced"),
    );
    for (i, (y, label, value, id, key)) in [
        (
            372,
            tr("Size"),
            l.font_size as f64,
            "reader:layout:size",
            "layout-size",
        ),
        (
            465,
            tr("Line spacing"),
            l.line_spacing as f64,
            "reader:layout:spacing",
            "layout-spacing",
        ),
        (
            557,
            tr("Margins"),
            l.margins.left as f64,
            "reader:layout:margins",
            "layout-margins",
        ),
    ]
    .into_iter()
    .enumerate()
    {
        put(chrome, screen, [85, y - 20, 320, 60], text(screen, &label));
        slider(chrome, screen, y, value, ranges(screen)[i], id, key);
    }
    put(chrome, screen, [85, 629, 320, 63], text(screen, &tr("Alignment")));
    let selected = if l.align_left {
        1
    } else if l.justify {
        2
    } else {
        0
    };
    for (i, name) in ["", "align.left", "align.justify"].into_iter().enumerate() {
        let mut tool = Tool::new(
            name,
            &format!("reader:layout:alignment:{i}"),
            48 * screen.w.min(screen.h) / 1264,
        );
        if i == 0 {
            tool.label = tr("Off");
        }
        tool.centered = true;
        tool.fill = true;
        tool.active = i == selected;
        // Segments fill their rectangle; toolbar text keeps a circular state.
        let pod = WidgetPod::new(Segment(tool))
            .focusable(true)
            .with_key(format!("layout-alignment-{i}"));
        put(chrome, screen, [428 + i as u32 * 259, 609, 258, 83], pod);
    }
}
struct Segment(Tool);
impl kaia_ui::Widget for Segment {
    kaia_ui::impl_as_any!();
    fn layout(&mut self, cx: &mut kaia_ui::LayoutCx, c: kaia_ui::Constraints) -> Size {
        self.0.layout(cx, c)
    }
    fn paint(&mut self, cx: &mut kaia_ui::PaintCx) {
        let active = self.0.active;
        let b = cx.bounds();
        cx.fill_rect(b, cx.color(if active { Tone::Ink } else { Tone::SurfaceAlt }));
        let tone = if active { Tone::Surface } else { Tone::Ink };
        if self.0.icon.is_empty() {
            let mut font = cx.theme.font.body_bold.clone();
            font.size = self.0.px as f32 * 0.72;
            let style = cx.theme.style(&font, tone);
            let t = cx.fonts.layout(&self.0.label, &style, b.w, Some(1), true);
            cx.text(
                &t,
                Point::new(
                    b.x + (b.w as i32 - t.width as i32) / 2,
                    b.y + (b.h as i32 - t.height as i32) / 2,
                ),
            );
        } else if let Ok(icon) = cx
            .icons
            .get(&self.0.icon, Size::new(self.0.px, self.0.px), cx.color(tone))
        {
            cx.blit_alpha(
                &icon,
                Point::new(
                    b.x + (b.w - self.0.px) as i32 / 2,
                    b.y + (b.h - self.0.px) as i32 / 2,
                ),
            );
        }
    }
    fn event(&mut self, cx: &mut kaia_ui::EventCx, e: &kaia_ui::UiEvent) -> kaia_ui::Handled {
        // The parent rebuilds the selection after this action.
        if matches!(e, kaia_ui::UiEvent::Gesture(kaia_input::Gesture::Tap { .. })) {
            cx.action(kaia_ui::AppAction {
                id: self.0.action.clone(),
                args: vec![],
            });
            kaia_ui::Handled::Yes
        } else {
            kaia_ui::Handled::No
        }
    }
}

pub(super) fn refresh(c: &Core, env: &mut SurfaceEnv<'_>) {
    let Some(layer) = &c.panel else { return };
    let Some(session) = &c.session else { return };
    let l = session.layout();
    for (i, (key, value)) in [
        ("layout-size", l.font_size as f64),
        ("layout-spacing", l.line_spacing as f64),
        ("layout-margins", l.margins.left as f64),
    ]
    .into_iter()
    .enumerate()
    {
        let [min, max, step] = ranges(env.screen)[i];
        layer
            .window
            .update::<crate::slider::Slider, _>(env.ctx, key, |s, cx| {
                s.set_value(value);
                cx.request_paint();
            });
        for (suffix, delta) in [("minus", -step), ("plus", step)] {
            layer
                .window
                .update::<Tool, _>(env.ctx, &format!("{key}-{suffix}"), |t, _| {
                    t.args = vec![Value::Number((value + delta).clamp(min, max))];
                });
        }
    }
    let face = panels::font_choice_label(panels::font_index(l.font_family.as_deref()));
    layer.window.update::<Tool, _>(env.ctx, "layout-font", |t, cx| {
        t.label = face.clone();
        cx.request_paint();
    });
}

fn fonts(services: &Services) -> Option<Rc<RefCell<kaia_gfx::text::FontSystem>>> {
    fn collect(root: &std::path::Path, depth: usize, dirs: &mut Vec<std::path::PathBuf>) {
        if depth == 0 || !root.is_dir() {
            return;
        }
        dirs.push(root.to_path_buf());
        if let Ok(entries) = std::fs::read_dir(root) {
            for e in entries.flatten() {
                if e.file_type().is_ok_and(|t| t.is_dir()) {
                    collect(&e.path(), depth - 1, dirs);
                }
            }
        }
    }
    let root = crate::services::resources_dir(&services.platform);
    let mut dirs = Vec::new();
    for root in [
        root.join("fonts"),
        root.join("kaia-fonts"),
        services.paths.install_dir.join("fonts"),
        services.paths.data_dir.join("fonts"),
    ] {
        collect(&root, 8, &mut dirs);
    }
    dirs.sort();
    dirs.dedup();
    kaia_gfx::text::FontSystem::load(&dirs, &[])
        .ok()
        .map(|f| Rc::new(RefCell::new(f)))
}
struct Sample {
    fonts: Option<Rc<RefCell<kaia_gfx::text::FontSystem>>>,
    family: String,
    size: f32,
    weight: u16,
    text: String,
    action: Option<String>,
    lines: usize,
}
impl kaia_ui::Widget for Sample {
    kaia_ui::impl_as_any!();
    fn layout(&mut self, _: &mut kaia_ui::LayoutCx, c: kaia_ui::Constraints) -> Size {
        c.max
    }
    fn paint(&mut self, cx: &mut kaia_ui::PaintCx) {
        let b = cx.bounds();
        let mut style = kaia_gfx::text::TextStyle::new(self.size);
        style.family = self.family.clone();
        style.weight = self.weight;
        style.color = cx.color(Tone::Ink);
        if self.fonts.is_none() {
            let text = cx.fonts.layout(&self.text, &style, b.w, Some(self.lines), true);
            cx.text(&text, Point::new(b.x, b.y));
            return;
        }
        let mut paint = |fonts: &mut kaia_gfx::text::FontSystem| {
            let t = fonts.layout(&self.text, &style, b.w, Some(self.lines), true);
            let mut pix = kaia_gfx::Pixmap::new(b.w, b.h, kaia_gfx::PixelFormat::Rgb24);
            pix.fill_rect(kaia_gfx::Rect::from_size(b.size()), cx.color(Tone::Surface));
            let y = if self.lines == 1 {
                (b.h as i32 - t.height as i32) / 2
            } else {
                0
            };
            fonts.draw_layout(&mut pix, &t, Point::new(0, y));
            cx.blit_pixmap(&pix, Point::new(b.x, b.y));
        };
        if let Some(fonts) = &self.fonts {
            paint(&mut fonts.borrow_mut());
        }
    }
    fn event(&mut self, cx: &mut kaia_ui::EventCx, e: &kaia_ui::UiEvent) -> kaia_ui::Handled {
        if matches!(e, kaia_ui::UiEvent::Gesture(kaia_input::Gesture::Tap { .. })) {
            if let Some(id) = &self.action {
                cx.action(kaia_ui::AppAction {
                    id: id.clone(),
                    args: vec![],
                });
                return kaia_ui::Handled::Yes;
            }
        }
        kaia_ui::Handled::No
    }
}
pub(super) fn font_list(services: &Services, screen: Size, current: usize, page: usize) -> Window {
    let choices = panels::font_choices();
    let count = choices.len();
    let page = page.min(count.saturating_sub(1) / 12);
    let rows = (count - page * 12).min(12);
    let h = rows as u32 * 91 + if count > 12 { 98 } else { 20 };
    let mut chrome = Chrome {
        panel_border: 3,
        size: screen,
        children: vec![],
        panels: vec![rect(screen, 347, 248, 628, h)],
    };
    let fonts = fonts(services);
    for (i, (label, face)) in choices.into_iter().enumerate().skip(page * 12).take(12) {
        let y = 258 + (i - page * 12) as u32 * 91;
        if current == i {
            put(
                &mut chrome,
                screen,
                [383, y + 20, 48, 48],
                icon(screen, "check", &format!("reader:layout:font:{i}"), "font-check"),
            );
        }
        put(
            &mut chrome,
            screen,
            [448, y, 505, 90],
            WidgetPod::new(Sample {
                fonts: fonts.clone(),
                family: face.unwrap_or("Noto Sans".into()),
                size: 34.0 * screen.w.min(screen.h) as f32 / 1264.0,
                weight: 400,
                text: label,
                action: Some(format!("reader:layout:font:{i}")),
                lines: 1,
            })
            .focusable(true)
            .with_key(format!("reader-layout-font-{i}")),
        );
        rule(&mut chrome, screen, [362, y + 90, 597, 1], Tone::Divider);
    }
    if count > 12 {
        if page > 0 {
            put(
                &mut chrome,
                screen,
                [400, 248 + h - 98, 190, 96],
                icon(
                    screen,
                    "chevron.up",
                    &format!("reader:fonts:page:{}", page - 1),
                    "font-previous",
                ),
            );
        }
        if (page + 1) * 12 < count {
            put(
                &mut chrome,
                screen,
                [650, 248 + h - 98, 190, 96],
                icon(
                    screen,
                    "chevron.down",
                    &format!("reader:fonts:page:{}", page + 1),
                    "font-next",
                ),
            );
        }
    }
    window(services, screen, chrome)
}
fn advanced_window(services: &Services, screen: Size, before: &TextLayout, draft: &TextLayout) -> Window {
    let mut chrome = dialog(screen);
    heading(&mut chrome, screen, &tr("Font settings"));
    put(
        &mut chrome,
        screen,
        [100, 180, 1000, 90],
        text(screen, &tr("Advanced")),
    );
    put(
        &mut chrome,
        screen,
        [125, 350, 1000, 70],
        text(screen, &tr("Compare size and weight before applying.")),
    );
    put(
        &mut chrome,
        screen,
        [125, 447, 310, 60],
        text(screen, &tr("Font")),
    );
    let face = panels::font_choice_label(panels::font_index(draft.font_family.as_deref()));
    put(
        &mut chrome,
        screen,
        [455, 427, 708, 80],
        dropdown(screen, &face, "reader:layout:fonts", "advanced-font"),
    );
    for (y, label, value, range, id, key) in [
        (
            558,
            tr("Size"),
            draft.font_size as f64,
            ranges(screen)[0],
            "reader:advanced:size",
            "advanced-size",
        ),
        (
            649,
            tr("Weight"),
            panels::font_weight_index(draft.font_weight) as f64,
            [0.0, 6.0, 1.0],
            "reader:advanced:weight",
            "advanced-weight",
        ),
    ] {
        put(&mut chrome, screen, [125, y - 20, 300, 60], text(screen, &label));
        slider(&mut chrome, screen, y, value, range, id, key);
    }
    let fonts = fonts(services);
    for (x, label, layout) in [(100, tr("Before"), before), (632, tr("After"), draft)] {
        rule(&mut chrome, screen, [x, 710, 532, 50], Tone::SurfaceAlt);
        put(
            &mut chrome,
            screen,
            [x + 20, 710, 490, 50],
            WidgetPod::new(kaia_ui::components::Frame::new(text(screen, &label)).fill(Tone::SurfaceAlt)),
        );
        put(&mut chrome,screen,[x+16,770,500,680],WidgetPod::new(Sample{
            fonts:fonts.clone(),family:layout.font_family.clone().unwrap_or("Noto Serif".into()),size:layout.font_size,weight:layout.css_weight() as u16,
            text:tr("Light falls across the page. The path follows a river past the old bridge. A traveller stops to read a letter, then folds it and walks on."),action:None,lines:40,
        }).with_key(if x==100 {"advanced-before"} else {"advanced-after"}));
        for bounds in [
            [x, 710, 532, 1],
            [x, 710, 1, 744],
            [x + 531, 710, 1, 744],
            [x, 1453, 532, 1],
        ] {
            rule(&mut chrome, screen, bounds, Tone::Ink);
        }
    }
    put(
        &mut chrome,
        screen,
        [789, 1524, 214, 80],
        flat(
            screen,
            &tr("Restore"),
            "reader:advanced:restore",
            "advanced-restore",
        ),
    );
    put(
        &mut chrome,
        screen,
        [1013, 1524, 175, 80],
        flat(screen, &tr("Apply"), "reader:advanced:apply", "advanced-apply"),
    );
    window(services, screen, chrome)
}
pub(super) fn command(
    rc: &Rc<RefCell<Core>>,
    cmd: &str,
    args: &[Value],
    env: &mut SurfaceEnv<'_>,
) -> Option<Outcome> {
    if cmd == "reader:advanced:open" {
        let mut c = rc.borrow_mut();
        let before = c.session.as_ref()?.layout().clone();
        let window = advanced_window(env.services, env.screen, &before, &before);
        let serial = super::next_serial(&mut c);
        c.advanced = Some(Advanced {
            before: before.clone(),
            draft: before,
            layer: Layer {
                serial,
                window: window.clone(),
            },
        });
        return Some(Outcome::Navigate(Nav::Push(panels::panel_surface(
            window,
            serial,
            rc.clone(),
        ))));
    }
    if let Some(page) = cmd
        .strip_prefix("reader:fonts:page:")
        .and_then(|s| s.parse().ok())
    {
        let mut c = rc.borrow_mut();
        let face = c
            .advanced
            .as_ref()
            .map(|a| a.draft.font_family.clone())
            .unwrap_or_else(|| c.session.as_ref().and_then(|s| s.layout().font_family.clone()));
        let window = font_list(
            env.services,
            env.screen,
            panels::font_index(face.as_deref()),
            page,
        );
        let serial = super::next_serial(&mut c);
        return Some(Outcome::Navigate(Nav::Replace(panels::panel_surface(
            window,
            serial,
            rc.clone(),
        ))));
    }
    if let Some(index) = cmd
        .strip_prefix("reader:layout:alignment:")
        .and_then(|s| s.parse::<usize>().ok())
    {
        let mut c = rc.borrow_mut();
        if index > 2 {
            return Some(Outcome::Handled);
        }
        if let Err(e) = c.change_layout(|l| {
            l.align_left = index == 1;
            l.justify = index == 2;
        }) {
            env.toast(e, 3.0);
        }
        let window = super::layout_window(&mut c, env);
        return Some(super::show_panel(rc, &mut c, env.ctx, window, true));
    }
    let is_font = cmd
        .strip_prefix("reader:layout:font:")
        .and_then(|s| s.parse::<usize>().ok());
    if !cmd.starts_with("reader:advanced:") && !(is_font.is_some() && rc.borrow().advanced.is_some()) {
        return None;
    }
    let mut c = rc.borrow_mut();
    if cmd == "reader:advanced:apply" {
        let draft = c.advanced.as_ref()?.draft.clone();
        let weight = draft.font_weight;
        let family = draft.font_family.clone();
        match c.change_layout(|l| *l = draft) {
            Ok(()) => {
                let key = crate::surfaces::settings::keys::font_weight_key(family.as_deref());
                env.services.settings.save(key, Value::from(weight as f64));
                super::save_settings(env);
                refresh(&c, env);
                return Some(Outcome::Navigate(Nav::Pop));
            }
            Err(e) => {
                env.toast(e, 3.0);
                return Some(Outcome::Handled);
            }
        }
    }
    let advanced = c.advanced.as_mut()?;
    match cmd {
        "reader:advanced:restore" => advanced.draft = advanced.before.clone(),
        "reader:advanced:size" => {
            let value = args.first()?.as_f64()?;
            if value.is_finite() {
                advanced.draft.font_size =
                    value.clamp(ranges(env.screen)[0][0], ranges(env.screen)[0][1]) as f32;
            }
        }
        "reader:advanced:weight" => {
            advanced.draft.font_weight = panels::font_weight_at(args.first()?.as_f64()?)
        }
        _ if is_font.is_some() => {
            let i = is_font.unwrap();
            advanced.draft.font_family = super::state::font_face(i).or_else(|| panels::user_face_at(i));
        }
        _ => return Some(Outcome::Handled),
    }
    let draft = &advanced.draft;
    advanced
        .layer
        .window
        .update::<Sample, _>(env.ctx, "advanced-after", |sample, cx| {
            sample.family = draft.font_family.clone().unwrap_or("Noto Serif".into());
            sample.size = draft.font_size;
            sample.weight = draft.css_weight() as u16;
            cx.request_paint();
        });
    let face = panels::font_choice_label(panels::font_index(draft.font_family.as_deref()));
    advanced
        .layer
        .window
        .update::<Tool, _>(env.ctx, "advanced-font", |tool, cx| {
            tool.label = face.clone();
            cx.request_paint();
        });
    for (key, value, range) in [
        ("advanced-size", draft.font_size as f64, ranges(env.screen)[0]),
        (
            "advanced-weight",
            panels::font_weight_index(draft.font_weight) as f64,
            [0.0, 6.0, 1.0],
        ),
    ] {
        advanced
            .layer
            .window
            .update::<crate::slider::Slider, _>(env.ctx, key, |slider, cx| {
                slider.set_value(value);
                cx.request_paint();
            });
        for (suffix, delta) in [("minus", -range[2]), ("plus", range[2])] {
            advanced
                .layer
                .window
                .update::<Tool, _>(env.ctx, &format!("{key}-{suffix}"), |tool, _| {
                    tool.args = vec![Value::Number((value + delta).clamp(range[0], range[1]))];
                });
        }
    }
    Some(if is_font.is_some() {
        Outcome::Navigate(Nav::Pop)
    } else {
        Outcome::Handled
    })
}
