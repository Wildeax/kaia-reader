//! Review 3 regressions for the plugin reader host: R3-H4 (plugin document work
//! on the app thread), R3-M1 (font units) and R3-M2 (new book flag).
#![cfg(feature = "lua")]
mod common;
use common::*;
use kaia_app::{App, HeadlessBackend};
use kaia_events::{Event, Settings, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

fn enable(app: &mut App<HeadlessBackend>, names: &[&str]) {
    app.process(vec![]).unwrap();
    let all: Vec<_> = app
        .services()
        .plugins
        .entries
        .iter()
        .map(|p| p.name.clone())
        .collect();
    for name in all {
        app.services_mut().settings.save(
            format!("plugins.on.{name}"),
            Value::from(names.contains(&name.as_str())),
        );
    }
    app.command("settings:set:plugins.enabled", &[Value::from(true)]);
    app.process(vec![]).unwrap();
    assert!(
        app.services().plugins.error().is_none(),
        "{:?}",
        app.services().plugins.error()
    );
}

/// Writes a plugin into the app's user plugin folder.
fn plugin(data: &Path, name: &str, main: &str) {
    let dir = data.join("data/plugins").join(format!("{name}.koplugin"));
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("_meta.lua"),
        format!("return {{name='{name}', fullname='{name}'}}"),
    )
    .unwrap();
    fs::write(dir.join("main.lua"), main).unwrap();
}

fn value(app: &App<HeadlessBackend>, plugin: &str, key: &str) -> Value {
    app.services()
        .plugins
        .plugin_value(plugin, key)
        .unwrap_or_default()
}

fn works(app: &App<HeadlessBackend>, name: &str) {
    let entry = app
        .services()
        .plugins
        .entries
        .iter()
        .find(|p| p.name == name)
        .unwrap_or_else(|| panic!("no plugin {name}"));
    assert_eq!(entry.state, "Works", "{entry:?}");
}

fn state_layout_font(app: &mut App<HeadlessBackend>, id: i64) -> f64 {
    app.broadcast_app(Event::new("SaveState"));
    let b = app.services().library.book(id).unwrap().unwrap();
    let path = app
        .services()
        .paths
        .data_dir
        .join("kaia/books")
        .join(format!("{}.json", b.partial_md5));
    Settings::open(&path)
        .read("layout")
        .get("font_size")
        .as_f64()
        .unwrap()
}

/// KOReader's scaleBySize on the test panel.
fn px(app: &App<HeadlessBackend>, units: f64) -> f64 {
    let s = app.screen();
    (units * f64::from(s.w.min(s.h)) / 600.0).ceil()
}

// ----- R3-H4 -----

/// The reviewer's probe plugin: it reads the reading position as an xpointer
/// when the reader is ready and again after a reflow. It also reads that
/// position's page and the page text.
const XPOINTER_PROBE: &str = r#"
local WidgetContainer = require("ui/widget/container/widgetcontainer")
local Probe = WidgetContainer:extend{ name = "xprobe" }
function Probe:read(prefix)
    local doc = self.ui.document
    local xp = doc:getXPointer()
    self[prefix .. "_xp"] = xp
    self[prefix .. "_page"] = doc:getPageFromXPointer(xp)
    self[prefix .. "_current"] = doc:getCurrentPage()
    self[prefix .. "_text"] = doc:getPageText(doc:getCurrentPage())
end
function Probe:onReaderReady() self:read("ready") end
function Probe:onDocumentRerendered() self:read("rerender") end
return Probe
"#;

/// An EPUB with `count` short paragraphs in one chapter, stored without compression.
fn long_epub(path: &Path, count: usize) {
    let mut body = String::with_capacity(count * 40);
    for n in 0..count {
        body.push_str(&format!("<p>Paragraph {n} of the long book.</p>\n"));
    }
    let chapter = format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<html xmlns=\"http://www.w3.org/1999/xhtml\"><head><title>Long</title></head><body><h1>Long</h1>\n{body}</body></html>\n"
    );
    let files: Vec<(&str, Vec<u8>)> = vec![
        ("mimetype", b"application/epub+zip".to_vec()),
        (
            "META-INF/container.xml",
            br#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#.to_vec(),
        ),
        (
            "content.opf",
            br#"<?xml version="1.0" encoding="utf-8"?><package xmlns="http://www.idpf.org/2007/opf" version="2.0" unique-identifier="id"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>Long book</dc:title><dc:identifier id="id">long-book</dc:identifier><dc:language>en</dc:language></metadata><manifest><item id="c1" href="chapter.xhtml" media-type="application/xhtml+xml"/><item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/></manifest><spine toc="ncx"><itemref idref="c1"/></spine></package>"#.to_vec(),
        ),
        (
            "toc.ncx",
            br#"<?xml version="1.0" encoding="utf-8"?><ncx xmlns="http://www.daisy.org/z3986/2005/ncx/" version="2005-1"><head><meta name="dtb:uid" content="long-book"/></head><docTitle><text>Long book</text></docTitle><navMap><navPoint id="p1" playOrder="1"><navLabel><text>Long</text></navLabel><content src="chapter.xhtml"/></navPoint></navMap></ncx>"#.to_vec(),
        ),
        ("chapter.xhtml", chapter.into_bytes()),
    ];
    fs::write(path, stored_zip(&files)).unwrap();
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

fn stored_zip(files: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for (name, data) in files {
        let offset = out.len() as u32;
        let crc = crc32(data);
        let size = data.len() as u32;
        let header = |sig: u32, central: bool, buf: &mut Vec<u8>| {
            buf.extend_from_slice(&sig.to_le_bytes());
            if central {
                buf.extend_from_slice(&20u16.to_le_bytes());
            }
            for v in [20u16, 0, 0, 0, 0x21] {
                buf.extend_from_slice(&v.to_le_bytes());
            }
            for v in [crc, size, size] {
                buf.extend_from_slice(&v.to_le_bytes());
            }
            buf.extend_from_slice(&(name.len() as u16).to_le_bytes());
            buf.extend_from_slice(&0u16.to_le_bytes());
            if central {
                for v in [0u16, 0, 0] {
                    buf.extend_from_slice(&v.to_le_bytes());
                }
                buf.extend_from_slice(&0u32.to_le_bytes());
                buf.extend_from_slice(&offset.to_le_bytes());
            }
            buf.extend_from_slice(name.as_bytes());
        };
        header(0x0403_4b50, false, &mut out);
        out.extend_from_slice(data);
        header(0x0201_4b50, true, &mut central);
    }
    let start = out.len() as u32;
    out.extend_from_slice(&central);
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    for v in [0u16, 0, files.len() as u16, files.len() as u16] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(&(central.len() as u32).to_le_bytes());
    out.extend_from_slice(&start.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

/// Runs the loop until `done` holds and returns the longest iteration.
fn slowest_until(
    app: &mut App<HeadlessBackend>,
    what: &str,
    done: impl Fn(&App<HeadlessBackend>) -> bool,
) -> Duration {
    let deadline = Instant::now() + Duration::from_secs(300);
    let mut slowest = Duration::ZERO;
    while !done(app) {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        let start = Instant::now();
        app.iterate(Some(Duration::from_millis(10))).unwrap();
        slowest = slowest.max(start.elapsed());
    }
    slowest
}

#[test]
fn plugin_xpointer_queries_keep_the_loop_responsive() {
    let _lock = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let data = scratch("r3-plugin-latency");
    let books = data.join("books");
    fs::create_dir_all(&books).unwrap();
    long_epub(&books.join("long.epub"), 40_000);
    plugin(&data, "xprobe", XPOINTER_PROBE);
    let mut app = make_app(&data, vec![books]);
    app.start().unwrap();
    wait_for(&mut app, "scan", |a| !a.scan_running());
    enable(&mut app, &["xprobe"]);
    let id = book_id(&app, "long.epub");

    let start = Instant::now();
    app.command(&format!("open_book:{id}"), &[]);
    let command = start.elapsed();
    let first = slowest_until(&mut app, "the first plugin query", |a| {
        value(a, "xprobe", "ready_xp").as_str().is_some()
    })
    .max(command);
    eprintln!("Plugin attach and first xpointer query: slowest loop iteration {first:?}");
    works(&app, "xprobe");
    let xp = value(&app, "xprobe", "ready_xp");
    assert!(xp.as_str().unwrap().starts_with('/'), "{xp:?}");
    assert_eq!(
        value(&app, "xprobe", "ready_page"),
        value(&app, "xprobe", "ready_current")
    );
    assert!(
        value(&app, "xprobe", "ready_text")
            .as_str()
            .is_some_and(|t| t.contains("Paragraph")),
        "{:?}",
        value(&app, "xprobe", "ready_text")
    );
    assert!(first < Duration::from_millis(100), "{first:?}");

    // A font size change lays the book out again, in both documents.
    wait_reader(&mut app);
    let bigger = px(&app, 30.0);
    let start = Instant::now();
    app.command("reader:layout:size", &[Value::Number(bigger)]);
    let command = start.elapsed();
    let reflow = slowest_until(&mut app, "the query after the reflow", |a| {
        value(a, "xprobe", "rerender_xp").as_str().is_some()
    })
    .max(command);
    eprintln!("Plugin query after a reflow: slowest loop iteration {reflow:?}");
    works(&app, "xprobe");
    assert_eq!(
        value(&app, "xprobe", "rerender_page"),
        value(&app, "xprobe", "rerender_current")
    );
    assert!(value(&app, "xprobe", "rerender_text")
        .as_str()
        .is_some_and(|t| t.contains("Paragraph")));
    assert!(reflow < Duration::from_millis(100), "{reflow:?}");
    app.shutdown();
}

// ----- R3-M1 -----

const FONT_PROBE: &str = r#"
local Event = require("ui/event")
local WidgetContainer = require("ui/widget/container/widgetcontainer")
local Probe = WidgetContainer:extend{ name = "fontprobe" }
function Probe:onReaderReady()
    self.read_size = self.ui.doc_settings:readSetting("font_size")
    self.configurable_size = self.ui.document.configurable.font_size
    self.ui.doc_settings:saveSetting("font_size", self.read_size)
    self.ui:handleEvent(Event:new("SetFontSize", self.read_size))
end
return Probe
"#;

#[test]
fn plugin_font_size_round_trip_keeps_the_native_default() {
    let _lock = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let data = scratch("r3-plugin-font");
    let books = library_with_books(&data);
    plugin(&data, "fontprobe", FONT_PROBE);
    let mut app = make_app(&data, vec![books]);
    app.start().unwrap();
    wait_for(&mut app, "scan", |a| !a.scan_running());
    enable(&mut app, &["fontprobe"]);
    let id = book_id(&app, "juliet.epub");
    app.command(&format!("open_book:{id}"), &[]);
    wait_for(&mut app, "the plugin read", |a| {
        value(a, "fontprobe", "read_size").as_f64().is_some()
    });
    wait_reader(&mut app);
    works(&app, "fontprobe");
    assert_eq!(value(&app, "fontprobe", "read_size").as_f64(), Some(36.0));
    assert_eq!(value(&app, "fontprobe", "configurable_size").as_f64(), Some(36.0));
    let font = state_layout_font(&mut app, id);
    assert_eq!(font, px(&app, 36.0));
    assert_eq!(font, 76.0, "the Libra Colour default");
    app.shutdown();
}

// ----- R3-M2 -----

fn real(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap()
}

#[test]
fn directory_defaults_apply_only_to_a_new_book() {
    let _lock = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let data = scratch("r3-plugin-new-book");
    let books = library_with_books(&data);
    let mut app = make_app(&data, vec![books.clone()]);
    app.start().unwrap();
    wait_for(&mut app, "scan", |a| !a.scan_running());
    // leaves.epub gets saved state before any directory rule exists.
    let read = book_id(&app, "leaves.epub");
    app.command(&format!("open_book:{read}"), &[]);
    wait_reader(&mut app);
    app.key(kaia_input::Key::RPgFwd).unwrap();
    wait_reader(&mut app);
    app.command("nav:home", &[]);
    wait_reader(&mut app);

    // A directory rule, as docsettingtweak's editor saves it.
    let lua_data = app.services().paths.data_dir.clone();
    let folder = real(&books).to_string_lossy().to_string();
    fs::write(
        lua_data.join("settings.reader.lua"),
        format!("return {{ [\"home_dir\"] = {folder:?} }}\n"),
    )
    .unwrap();
    fs::create_dir_all(lua_data.join("settings")).unwrap();
    fs::write(
        lua_data.join("settings/directory_defaults.lua"),
        format!("return {{ [{folder:?}] = {{ font_size = 30 }} }}\n"),
    )
    .unwrap();
    enable(&mut app, &["docsettingtweak"]);

    let new = book_id(&app, "juliet.epub");
    app.command(&format!("open_book:{new}"), &[]);
    wait_reader(&mut app);
    app.process(vec![]).unwrap();
    wait_reader(&mut app);
    works(&app, "docsettingtweak");
    assert_eq!(state_layout_font(&mut app, new), px(&app, 30.0));
    app.command("nav:home", &[]);
    wait_reader(&mut app);

    app.command(&format!("open_book:{read}"), &[]);
    wait_reader(&mut app);
    app.process(vec![]).unwrap();
    wait_reader(&mut app);
    works(&app, "docsettingtweak");
    assert_eq!(state_layout_font(&mut app, read), px(&app, 36.0));
    app.shutdown();
}
