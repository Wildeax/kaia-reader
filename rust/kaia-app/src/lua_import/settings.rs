//! Only settings with a Rust consumer cross into settings.json.
use super::Result;
use kaia_events::{Table, Value};
use std::path::Path;

pub(super) fn mapped(root: &Value, source: &Path) -> Result<Vec<(&'static str, Value)>> {
    let mut out = Vec::new();
    if let Some(font) = root.get("cre_font").as_str().filter(|s| !s.is_empty()) {
        out.push(("reading.font_family", font.into()));
    }
    for (lua, rust, min, max, divisor) in [
        ("copt_font_size", "reading.font_size", 12.0, 40.0, 1.0),
        ("copt_t_page_margin", "reading.margin_top", 0.0, 140.0, 1.0),
        ("copt_b_page_margin", "reading.margin_bottom", 0.0, 140.0, 1.0),
        ("copt_line_spacing", "reading.line_spacing", 50.0, 300.0, 100.0),
        ("full_refresh_count", "reading.full_refresh_pages", 0.0, 10.0, 1.0),
        ("frontlight_intensity", "display.frontlight", 0.0, 100.0, 1.0),
        ("frontlight_warmth", "display.warmth", 0.0, 100.0, 1.0),
    ] {
        if let Some(n) = root.get(lua).as_f64().filter(|n| n.is_finite()) {
            out.push((rust, (n.clamp(min, max) / divisor).into()));
        }
    }
    for (index, key) in [(1i64, "reading.margin_left"), (2, "reading.margin_right")] {
        if let Some(n) = root
            .get("copt_h_page_margins")
            .get(index)
            .as_f64()
            .filter(|n| n.is_finite())
        {
            out.push((key, n.clamp(0.0, 140.0).into()));
        }
    }
    if let Some(v) = root.get("input_invert_page_turn_keys").as_bool() {
        out.push(("reading.invert_page_buttons", v.into()));
    }
    if let Some(v) = root
        .get("highlight_drawing_style")
        .as_str()
        .filter(|s| ["lighten", "underscore", "strikeout", "invert"].contains(s))
    {
        out.push(("reading.highlight_drawer", v.into()));
    }
    if let Some(v) = root
        .get("highlight_color")
        .as_str()
        .filter(|s| kaia_book::HIGHLIGHT_COLORS.contains(s))
    {
        out.push(("reading.highlight_color", v.into()));
    }
    if let Some(order) = root
        .get("dicts_order")
        .as_table()
        .filter(|_| source.join("data/dict").is_dir())
    {
        let dictionaries = kaia_dict::Library::open_dir(source.join("data/dict"))?;
        let mut ranked = Vec::new();
        for dictionary in dictionaries.dictionaries() {
            for (path, rank) in order {
                let kaia_events::Key::Str(path) = path else {
                    continue;
                };
                let p = Path::new(path);
                let p = if p.is_absolute() {
                    p.to_owned()
                } else {
                    source.join(p)
                };
                if std::fs::canonicalize(p).ok() == std::fs::canonicalize(dictionary.path()).ok() {
                    if let Some(n) = rank.as_i64() {
                        ranked.push((n, dictionary.name().to_owned()));
                    }
                }
            }
        }
        ranked.sort();
        if !ranked.is_empty() {
            out.push((
                "dictionary.order",
                Value::array(ranked.into_iter().map(|(_, name)| name.into())),
            ));
        }
    }
    Ok(out)
}

pub(super) fn plugins(root: &Value) -> Value {
    let mut out = Table::new();
    for key in [
        "statistics",
        "readtimer",
        "exporter",
        "vocabulary_builder",
        "httpinspector",
        "autoturn_timeout_seconds",
        "autoturn_distance",
        "profiles_autoexec",
        "calibre_wireless_url",
        "calibre_wireless_password",
        "calibre_wireless_default_format",
        "inbox_dir",
        "language_japanese_text_scan_length",
        "language_japanese_text_conversions",
    ] {
        let value = root.get(key);
        if !value.is_nil() {
            out.insert(key.into(), value.clone());
        }
    }
    Value::Table(out)
}
