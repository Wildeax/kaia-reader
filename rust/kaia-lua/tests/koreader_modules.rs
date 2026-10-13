//! KOReader's own pure modules, loaded unchanged from the KOReader tree.
//!
//! Each test runs assertions written in Lua, so a failure shows the Lua line
//! and traceback. The modules are util, ffi/util, dump, datetime, sort,
//! luasettings, docsettings, gettext and logger. Only datastorage is a Kaia
//! shim, and it gives the data folder that the host was created with.

mod common;

use std::fs;

use common::{host, koreader_dir, repo_l10n, serial, TempDir};
use kaia_events::value::Value;
use kaia_lua::LuaHost;

/// Runs a Lua chunk with asserts. Any failed assert fails the test.
fn check(host: &LuaHost, name: &str, source: &str) {
    if let Err(err) = host.exec(name, source) {
        panic!("{name} failed: {err}");
    }
}

/// The module files that the host loads are KOReader's, not shims.
#[test]
fn pure_modules_load_from_the_koreader_tree() {
    let _guard = serial();
    let data = TempDir::new("provenance");
    let host = host(&data);
    let root = koreader_dir();
    let modules = [
        "util",
        "ffi/util",
        "dump",
        "datetime",
        "sort",
        "luasettings",
        "docsettings",
        "gettext",
        "logger",
    ];
    for module in modules {
        let found = host
            .eval(
                "provenance.lua",
                &format!("return package.searchpath('{module}', package.path)"),
            )
            .unwrap();
        let path = found
            .as_str()
            .unwrap_or_else(|| panic!("{module} not found"))
            .to_owned();
        assert!(
            path.starts_with(root.to_str().unwrap())
                && !path.contains(&common::shim_dir().display().to_string()),
            "{module} resolves to {path}"
        );
        host.require(module)
            .unwrap_or_else(|e| panic!("require {module}: {e}"));
    }
    let shim = host
        .eval(
            "provenance.lua",
            "return package.searchpath('datastorage', package.path)",
        )
        .unwrap();
    assert!(shim
        .as_str()
        .unwrap()
        .starts_with(common::shim_dir().to_str().unwrap()));
}

#[test]
fn util() {
    let _guard = serial();
    let data = TempDir::new("util");
    let host = host(&data);
    check(
        &host,
        "util.lua",
        r#"
        local util = require("util")
        assert(util.trim("  a b  ") == "a b")
        assert(util.stripPunctuation('"hello world?"') == "hello world")
        assert(util.stripPunctuation(nil) == nil)
        local dir, name = util.splitFilePathName("/books/shelf/book.epub")
        assert(dir == "/books/shelf/" and name == "book.epub")
        local words = {}
        for w in util.gsplit("one, two  three", "%s+", false) do words[#words + 1] = w end
        assert(#words == 3 and words[1] == "one," and words[3] == "three")
        assert(util.htmlEscape([[<a href="x">&]]) == "&lt;a href=&quot;x&quot;&gt;&amp;")
        assert(util.tableEquals({ 1, { x = 2 } }, { 1, { x = 2 } }))
        assert(not util.tableEquals({ 1, 2 }, { 1, 3 }))
        assert(util.ltrim("  left") == "left" and util.rtrim("right  ") == "right")
        "#,
    );
}

#[test]
fn ffi_util() {
    let _guard = serial();
    let data = TempDir::new("ffi-util");
    let source = TempDir::new("ffi-util-src");
    let host = host(&data);
    let from = source.path().join("from.txt");
    let to = source.path().join("to.txt");
    fs::write(&from, b"copied bytes").unwrap();
    host.set_global("FROM", &Value::Str(from.display().to_string()))
        .unwrap();
    host.set_global("TO", &Value::Str(to.display().to_string()))
        .unwrap();
    check(
        &host,
        "ffi_util.lua",
        r#"
        local ffiutil = require("ffi/util")
        assert(ffiutil.template("%1 and %2", "a", "b") == "a and b")
        assert(ffiutil.joinPath("dir", "file") == "dir/file")
        assert(ffiutil.getTimestamp() > 1700000000)
        -- copyFile returns nil on success and an error text on failure.
        assert(ffiutil.copyFile(FROM, TO) == nil)
        local seen = {}
        for k in ffiutil.orderedPairs({ b = 1, a = 2, c = 3 }) do seen[#seen + 1] = k end
        assert(table.concat(seen, ",") == "a,b,c")
        assert(ffiutil.copyFile(FROM .. ".missing", TO) ~= nil)
        "#,
    );
    assert_eq!(fs::read(&to).unwrap(), b"copied bytes");
}

#[test]
fn dump_round_trips_a_table() {
    let _guard = serial();
    let data = TempDir::new("dump");
    let host = host(&data);
    check(
        &host,
        "dump.lua",
        r#"
        local dump = require("dump")
        local original = { name = "kaia", list = { 1, 2.5, "three" }, flag = true, [7] = false }
        local text = dump(original, nil, true)
        assert(type(text) == "string")
        assert(dump(original, nil, true) == text, "ordered output is stable")
        local copy = loadstring("return " .. text)()
        assert(copy.name == "kaia" and copy.list[2] == 2.5 and copy.list[3] == "three")
        assert(copy.flag == true and copy[7] == false)
        "#,
    );
}

#[test]
fn datetime() {
    let _guard = serial();
    let data = TempDir::new("datetime");
    let host = host(&data);
    check(
        &host,
        "datetime.lua",
        r#"
        local datetime = require("datetime")
        assert(datetime.secondsToClock(3661) == "01:01:01", datetime.secondsToClock(3661))
        assert(datetime.secondsToClock(3661, true) == "01:01", datetime.secondsToClock(3661, true))
        assert(datetime.secondsToClock(nil) == "--:--:--")
        assert(datetime.secondsToClock(0 / 0) == "00:00:00")
        local at = datetime.stringToSeconds("2026-10-13 12:30:00")
        assert(type(at) == "number" and at > 0)
        assert(datetime.stringToSeconds("2026-10-13 12:31:00") - at == 60)
        assert(type(datetime.secondsToDate(at)) == "string")
        "#,
    );
}

#[test]
fn sort() {
    let _guard = serial();
    let data = TempDir::new("sort");
    let host = host(&data);
    check(
        &host,
        "sort.lua",
        r#"
        local sort = require("sort")
        local names = { "chapter10.xhtml", "chapter2.xhtml", "chapter1.xhtml", "appendix.xhtml" }
        table.sort(names, sort.natsort_cmp())
        assert(table.concat(names, " ") == "appendix.xhtml chapter1.xhtml chapter2.xhtml chapter10.xhtml",
            table.concat(names, " "))
        "#,
    );
}

#[test]
fn luasettings_save_flush_and_read_back() {
    let _guard = serial();
    let data = TempDir::new("luasettings");
    let host = host(&data);
    // LuaSettings writes into an existing folder, so the data folder is used.
    let path = data.path().join("settings.test.lua");
    host.set_global("SETTINGS_PATH", &Value::Str(path.display().to_string()))
        .unwrap();
    check(
        &host,
        "luasettings_write.lua",
        r#"
        local LuaSettings = require("luasettings")
        local settings = LuaSettings:open(SETTINGS_PATH)
        settings:saveSetting("name", "kaia")
        settings:saveSetting("ratio", 0.25)
        settings:saveSetting("layout", { columns = { 1, 2 }, dark = true })
        settings:flush()
        "#,
    );
    assert!(path.is_file(), "settings file not written to {}", path.display());
    check(
        &host,
        "luasettings_read.lua",
        r#"
        local LuaSettings = require("luasettings")
        local settings = LuaSettings:open(SETTINGS_PATH)
        assert(settings:readSetting("name") == "kaia")
        assert(settings:readSetting("ratio") == 0.25)
        assert(settings:readSetting("layout").columns[2] == 2 and settings:readSetting("layout").dark == true)
        assert(settings:readSetting("absent", "fallback") == "fallback")
        "#,
    );
}

#[test]
fn docsettings_sidecar_goes_to_the_data_folder() {
    let _guard = serial();
    let data = TempDir::new("docsettings");
    let books = TempDir::new("docsettings-books");
    let host = host(&data);
    let book = books.path().join("leaves.epub");
    fs::copy(common::repo_root().join("test/leaves.epub"), &book).unwrap();
    host.set_global("BOOK", &Value::Str(book.display().to_string()))
        .unwrap();
    host.set_global("DATA", &Value::Str(data.path().display().to_string()))
        .unwrap();
    check(
        &host,
        "docsettings_write.lua",
        r#"
        local DataStorage = require("datastorage")
        local DocSettings = require("docsettings")
        assert(DataStorage:getDataDir() == DATA, "data dir " .. DataStorage:getDataDir())
        G_reader_settings:saveSetting("document_metadata_folder", "dir")
        local sidecar_dir = DocSettings:getSidecarDir(BOOK)
        assert(sidecar_dir:sub(1, #DATA) == DATA, "sidecar dir " .. sidecar_dir)
        local ds = DocSettings:open(BOOK)
        ds:saveSetting("percent_finished", 0.5)
        ds:saveSetting("summary", { status = "reading" })
        ds:flush()
        local found = DocSettings:findSidecarFile(BOOK)
        assert(found and found:sub(1, #DATA) == DATA, "sidecar file " .. tostring(found))
        "#,
    );
    // Nothing is written next to the book.
    assert!(!books.path().join("leaves.sdr").exists());
    let sidecars = fs::read_dir(data.path().join("docsettings"))
        .map(|d| d.count())
        .unwrap_or(0);
    assert!(sidecars > 0, "no sidecar under the data folder");
    check(
        &host,
        "docsettings_read.lua",
        r#"
        local DocSettings = require("docsettings")
        local ds = DocSettings:open(BOOK)
        assert(ds:readSetting("percent_finished") == 0.5)
        assert(ds:readSetting("summary").status == "reading")
        "#,
    );
}

#[test]
fn gettext_uses_the_repository_translations_with_english_fallback() {
    let _guard = serial();
    let data = TempDir::new("gettext");
    let host = host(&data);
    host.set_global("L10N", &Value::Str(repo_l10n().display().to_string()))
        .unwrap();
    check(
        &host,
        "gettext.lua",
        r#"
        local GetText = require("gettext")
        GetText.dirname = L10N
        local _ = require("gettext")
        assert(_("Cancel") == "Cancel")
        assert(GetText.changeLang("de") == true)
        assert(_("Cancel") == "Abbrechen", _("Cancel"))
        -- A string without a translation falls back to the English text.
        assert(_("kaia-lua test string with no translation") == "kaia-lua test string with no translation")
        -- An unknown language falls back to English.
        assert(GetText.changeLang("xx_NOPE") ~= true)
        assert(_("Cancel") == "Cancel")
        assert(GetText.changeLang("C") ~= false)
        assert(_("Cancel") == "Cancel")
        "#,
    );
}

#[test]
fn logger_writes_without_error() {
    let _guard = serial();
    let data = TempDir::new("logger");
    let host = host(&data);
    check(
        &host,
        "logger.lua",
        r#"
        local logger = require("logger")
        logger.info("kaia-lua test line", 1, { a = true })
        logger.warn("kaia-lua warning line")
        logger.dbg("kaia-lua debug line")
        assert(type(logger.warn) == "function" and type(logger.info) == "function")
        "#,
    );
}

/// One chunk that uses several modules together, as a plugin would.
#[test]
fn modules_work_together_in_one_script() {
    let _guard = serial();
    let data = TempDir::new("combined");
    let host = host(&data);
    host.set_global("L10N", &Value::Str(repo_l10n().display().to_string()))
        .unwrap();
    host.set_global(
        "SETTINGS_PATH",
        &Value::Str(data.path().join("combined.lua").display().to_string()),
    )
    .unwrap();
    let summary = host
        .eval(
            "combined.lua",
            r#"
            local util = require("util")
            local dump = require("dump")
            local sort = require("sort")
            local datetime = require("datetime")
            local LuaSettings = require("luasettings")
            local GetText = require("gettext")
            local logger = require("logger")
            GetText.dirname = L10N
            local _ = require("gettext")

            local books = { "vol10.epub", "vol2.epub", "vol1.epub" }
            table.sort(books, sort.natsort_cmp())

            local settings = LuaSettings:open(SETTINGS_PATH)
            settings:saveSetting("books", books)
            settings:saveSetting("title", util.trim("  Kaia  "))
            settings:flush()
            local reopened = LuaSettings:open(SETTINGS_PATH)

            GetText.changeLang("de")
            local cancel = _("Cancel")
            GetText.changeLang("C")
            logger.info("combined script")

            local _, name = util.splitFilePathName(books[1])
            local copy = loadstring("return " .. dump(reopened:readSetting("books"), nil, true))()
            return {
                first = copy[1],
                last = copy[3],
                title = reopened:readSetting("title"),
                cancel = cancel,
                clock = datetime.secondsToClock(3600),
                name = name,
            }
            "#,
        )
        .unwrap();
    assert_eq!(summary.get("first").as_str(), Some("vol1.epub"));
    assert_eq!(summary.get("last").as_str(), Some("vol10.epub"));
    assert_eq!(summary.get("title").as_str(), Some("Kaia"));
    assert_eq!(summary.get("cancel").as_str(), Some("Abbrechen"));
    assert_eq!(summary.get("clock").as_str(), Some("01:00:00"));
    assert_eq!(summary.get("name").as_str(), Some("vol1.epub"));
}
