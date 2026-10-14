//! Writes a CREngine comparison snapshot. Driven by tools/kaia/cre_compare.py.
use std::{error::Error, fs, path::Path};

use kaia_doc::{
    cre::{CreDocument, CreOptions, Kind},
    Document, Layout, Margins,
};
use kaia_gfx::{png::write_pixmap, PixelFormat};
use serde_json::{json, Value};

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 5 {
        return Err("usage: cre_snapshot EMULATOR BOOK SETTINGS_JSON OUTPUT".into());
    }
    let emulator = Path::new(&args[1]);
    let settings: Value = serde_json::from_slice(&fs::read(&args[3])?)?;
    let number = |key: &str| -> Result<f32, Box<dyn Error>> {
        Ok(settings[key].as_f64().ok_or_else(|| format!("missing {key}"))? as f32)
    };
    let output = Path::new(&args[4]);
    fs::create_dir_all(output)?;
    let options = CreOptions {
        data_dir: emulator.join("data"),
        font_dirs: vec![emulator.join("fonts")],
    };
    let mut doc = CreDocument::open_as_with_options(Path::new(&args[2]), Kind::Epub, &options)?;
    let mut layout = Layout::new(number("width")?, number("height")?, number("font_size")?);
    layout.font_family = Some(settings["font_family"].as_str().ok_or("font family")?.into());
    layout.font_weight = Some(number("font_weight")? as u32);
    layout.margins = Some(Margins {
        left: number("margin_left")?,
        top: number("margin_top")?,
        right: number("margin_right")?,
        bottom: number("margin_bottom")?,
    });
    layout.line_height = Some(number("line_percent")? / 100.0);
    layout.css_extra = settings["css_extra"].as_str().ok_or("css extra")?.into();
    doc.set_layout(&layout)?;
    doc.set_hyphenation(settings["hyphenation"].as_str().filter(|s| *s != "@none"))?;
    doc.set_embedded_styles(settings["embedded_styles"].as_bool().ok_or("embedded styles")?)?;
    let count = doc.page_count() as u32;
    if count < 2 {
        return Err("comparison corpus requires at least two pages".into());
    }
    let mut pages = vec![0, 1, 2, count / 4, count / 2, count * 3 / 4, count - 2];
    pages.retain(|&p| p + 1 < count);
    pages.sort_unstable();
    pages.dedup();
    let mut snapshots = Vec::new();
    for page in pages {
        let start = doc.page_xpointer(page)?;
        let end = doc.page_xpointer(page + 1)?;
        let text = doc.range_text(&start, &end)?;
        let pixmap = doc.render(page, 1.0, PixelFormat::Gray8)?;
        write_pixmap(&pixmap, &output.join(format!("page-{page}.png")))?;
        fs::write(output.join(format!("page-{page}.gray")), pixmap.data())?;
        snapshots.push(json!({"page": page, "start": start, "end": end, "text": text}));
    }
    fs::write(
        output.join("snapshot.json"),
        serde_json::to_vec_pretty(&json!({
            "page_count": count, "pages": snapshots, "settings": settings,
        }))?,
    )?;
    Ok(())
}
