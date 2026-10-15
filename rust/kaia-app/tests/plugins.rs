#![cfg(feature = "lua")]
mod common;
use common::*;
use kaia_events::Value;
use kaia_input::Key;

fn enable(app: &mut kaia_app::App<kaia_app::HeadlessBackend>, names: &[&str]) {
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
        app.services().plugins.running(),
        "{:?}",
        app.services().plugins.error()
    );
    assert!(
        app.services().plugins.error().is_none(),
        "{:?}",
        app.services().plugins.error()
    );
}

#[test]
fn settings_plugin_menu_opens_a_foreign_dialog_and_tap_closes_it() {
    let _lock = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let data = scratch("plugin-settings");
    let mut app = make_app(&data, vec![]);
    app.start().unwrap();
    assert!(!app.services().plugins.running());
    app.command("nav:settings", &[]);
    app.command("settings:extensions", &[]);
    enable(&mut app, &["hello", "exporter", "statistics"]);
    assert!(app.services().plugins.menus.iter().any(|m| m.id == "hello_world"));
    app.command("plugins:detail:hello", &[]);
    app.process(vec![]).unwrap();
    kaia_gfx::golden::assert_matches_golden(app.frame(), &golden("plugin_settings.png"), 14);
    tap_key(&mut app, "plugins:menu:hello_world");
    kaia_gfx::golden::assert_matches_golden(app.frame(), &golden("plugin_hello.png"), 14);
    app.process(vec![]).unwrap();
    assert_eq!(app.services().plugins.foreign_window_count(), 1);
    app.tap(kaia_gfx::Point::new(632, 840)).unwrap();
    assert_eq!(app.services().plugins.foreign_window_count(), 0);
    app.shutdown();
}

#[test]
fn reader_plugin_gets_page_update_from_app_page_turn() {
    let _lock = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let data = scratch("plugin-page");
    let books = library_with_books(&data);
    let mut app = make_app(&data, vec![books]);
    app.start().unwrap();
    wait_for(&mut app, "scan", |a| !a.scan_running());
    enable(&mut app, &["statistics", "exporter"]);
    let id = book_id(&app, "2col.pdf");
    app.command(&format!("open_book:{id}"), &[]);
    wait_reader(&mut app);
    assert!(
        app.services()
            .plugins
            .entries
            .iter()
            .any(|p| p.name == "statistics" && p.state == "Works"),
        "{:?}",
        app.services().plugins.entries
    );
    let before = app
        .services()
        .plugins
        .plugin_value("statistics", "curr_page")
        .unwrap()
        .as_i64()
        .unwrap();
    app.key(Key::RPgFwd).unwrap();
    wait_reader(&mut app);
    assert_eq!(
        app.services()
            .plugins
            .plugin_value("statistics", "curr_page")
            .unwrap()
            .as_i64(),
        Some(before + 1)
    );
    app.command("plugins:menu:statistics", &[]);
    app.process(vec![]).unwrap();
    assert!(app.services().plugins.foreign_window_count() > 0);
    app.shutdown();
}

fn copy_tree(source: &std::path::Path, target: &std::path::Path) {
    std::fs::create_dir_all(target).unwrap();
    for entry in std::fs::read_dir(source).unwrap() {
        let p = entry.unwrap().path();
        let dest = target.join(p.file_name().unwrap());
        if p.is_dir() {
            copy_tree(&p, &dest);
        } else {
            std::fs::copy(p, dest).unwrap();
        }
    }
}

#[test]
fn safe_mode_skips_user_community_plugin() {
    let _lock = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let data = scratch("plugin-safe");
    copy_tree(
        &repo_root().join("rust/kaia-lua/tests/community/readingstreak.koplugin"),
        &data.join("data/plugins/readingstreak.koplugin"),
    );
    let mut app = make_app(&data, vec![]);
    app.services_mut().plugins.startup_safe_mode = true;
    app.start().unwrap();
    enable(&mut app, &["hello", "readingstreak"]);
    assert!(app
        .services()
        .plugins
        .entries
        .iter()
        .any(|p| p.name == "readingstreak" && p.state == "Disabled by safe mode"));
    app.shutdown();
}

#[test]
fn plugin_error_is_failed_in_extensions() {
    let _lock = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let data = scratch("plugin-failure");
    let plugin = data.join("data/plugins/broken.koplugin");
    std::fs::create_dir_all(&plugin).unwrap();
    std::fs::write(
        plugin.join("_meta.lua"),
        "return {name='broken', fullname='Broken test'}",
    )
    .unwrap();
    std::fs::write(plugin.join("main.lua"), "error('app plugin fixture failed')").unwrap();
    let mut app = make_app(&data, vec![]);
    app.start().unwrap();
    enable(&mut app, &["hello", "broken"]);
    let failure = app
        .services()
        .plugins
        .entries
        .iter()
        .find(|p| p.name == "broken")
        .unwrap();
    assert!(failure.state.contains("app plugin fixture failed"), "{failure:?}");
    app.command("plugins:menu:hello_world", &[]);
    app.process(vec![]).unwrap();
    assert_eq!(app.services().plugins.foreign_window_count(), 1);
    app.shutdown();
}

#[test]
fn community_readingstreak_records_app_page_turns_and_reopens() {
    let _lock = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let data = scratch("plugin-streak");
    let books = library_with_books(&data);
    copy_tree(
        &repo_root().join("rust/kaia-lua/tests/community/readingstreak.koplugin"),
        &data.join("data/plugins/readingstreak.koplugin"),
    );
    std::fs::create_dir_all(data.join("data/settings")).unwrap();
    std::fs::write(
        data.join("data/settings/reading_streak.lua"),
        "return { daily_page_threshold = 100, auto_track = true }",
    )
    .unwrap();
    let mut app = make_app(&data, vec![books]);
    app.start().unwrap();
    wait_for(&mut app, "scan", |a| !a.scan_running());
    enable(&mut app, &["readingstreak"]);
    let id = book_id(&app, "2col.pdf");
    app.command(&format!("open_book:{id}"), &[]);
    wait_reader(&mut app);
    let before = app
        .services()
        .plugins
        .plugin_value("readingstreak", "settings")
        .unwrap()
        .get("daily_progress")
        .get("pages")
        .as_i64()
        .unwrap_or(0);
    app.key(Key::RPgFwd).unwrap();
    wait_reader(&mut app);
    assert_eq!(
        app.services()
            .plugins
            .plugin_value("readingstreak", "settings")
            .unwrap()
            .get("daily_progress")
            .get("pages")
            .as_i64(),
        Some(before + 1)
    );
    app.command("nav:home", &[]);
    app.process(vec![]).unwrap();
    app.command(&format!("open_book:{id}"), &[]);
    wait_reader(&mut app);
    assert_eq!(
        app.services()
            .plugins
            .plugin_value("readingstreak", "settings")
            .unwrap()
            .get("daily_progress")
            .get("pages")
            .as_i64(),
        Some(before + 1)
    );
    app.shutdown();
}

#[test]
fn dictionary_lookup_reaches_vocabulary_builder() {
    let _lock = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let data = scratch("plugin-dictionary");
    let books = library_with_books(&data);
    let mut app = make_app(&data, vec![books]);
    app.start().unwrap();
    wait_for(&mut app, "scan", |a| !a.scan_running());
    std::fs::write(
        data.join("data/settings.reader.lua"),
        "return { vocabulary_builder = {enabled=true, with_context=false} }",
    )
    .unwrap();
    enable(&mut app, &["vocabbuilder"]);
    let id = book_id(&app, "2col.pdf");
    app.command(&format!("open_book:{id}"), &[]);
    wait_reader(&mut app);
    app.command("reader:dictionary-word", &[Value::from("reader")]);
    app.process(vec![]).unwrap();
    let db = rusqlite::Connection::open(data.join("data/settings/vocabulary_builder.sqlite3")).unwrap();
    let words: i64 = db
        .query_row("SELECT count(*) FROM vocabulary WHERE word = 'reader'", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(words, 1);
    tap_key(&mut app, "dictionary-plugins");
    assert!(app.services().plugins.foreign_window_count() > 0);
    app.shutdown();
}

#[test]
fn plugin_device_requests_use_platform_and_idle_flags() {
    let _lock = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let data = scratch("plugin-device");
    let plugin = data.join("data/plugins/deviceprobe.koplugin");
    std::fs::create_dir_all(&plugin).unwrap();
    std::fs::write(plugin.join("_meta.lua"), "return {fullname='Device probe'}").unwrap();
    std::fs::write(
        plugin.join("main.lua"),
        r#"
        local P = require('ui/widget/widget'):extend{name='deviceprobe'}
        function P:init() self.ui.menu:registerToMainMenu(self) end
        function P:onSuspend() self.suspends = (self.suspends or 0) + 1 end
        function P:onResume() self.resumes = (self.resumes or 0) + 1 end
        function P:addToMainMenu(items)
            items.device_probe = {text='Set light', callback=function()
                local p = require('device'):getPowerDevice()
                self.battery = p:getCapacity()
                self.charging = p:isCharging()
                p:setIntensity(42); p:setWarmth(63)
                require('pluginshare').pause_auto_suspend = true
            end}
            items.sleep_probe = {text='Sleep', callback=function() require('ui/uimanager'):suspend() end}
        end
        return P
    "#,
    )
    .unwrap();
    let mut app = make_app(&data, vec![]);
    app.start().unwrap();
    enable(&mut app, &["deviceprobe", "autodim", "systemstat", "batterystat"]);
    for menu in ["system_statistics", "battery_statistics"] {
        app.command(&format!("plugins:menu:{menu}"), &[]);
        app.process(vec![]).unwrap();
        assert!(
            app.services().plugins.foreign_window_count() > 0,
            "{:?}",
            app.services().plugins.entries
        );
        app.key(Key::Back).unwrap();
    }
    app.command("plugins:menu:device_probe", &[]);
    app.process(vec![]).unwrap();
    assert_eq!(
        app.services()
            .plugins
            .plugin_value("deviceprobe", "battery")
            .unwrap()
            .as_i64(),
        Some(100)
    );
    assert_eq!(app.services().platform.frontlight.intensity(), 42);
    assert_eq!(app.services().platform.frontlight.warmth(), 63);
    assert!(app.services().plugins.idle_paused());
    app.command("plugins:menu:sleep_probe", &[]);
    app.process(vec![]).unwrap();
    assert!(app.sleeping());
    app.key(Key::Power).unwrap();
    assert!(!app.sleeping());
    app.process(vec![]).unwrap();
    for name in ["systemstat", "batterystat"] {
        assert!(app
            .services()
            .plugins
            .entries
            .iter()
            .any(|p| p.name == name && p.state == "Works"));
    }
    for key in ["suspends", "resumes"] {
        assert_eq!(
            app.services()
                .plugins
                .plugin_value("deviceprobe", key)
                .unwrap()
                .as_i64(),
            Some(1)
        );
    }
    app.shutdown();
}

#[test]
fn reader_paints_perception_overlay_and_plugin_footer() {
    let _lock = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let data = scratch("plugin-overlay");
    let books = library_with_books(&data);
    std::fs::create_dir_all(data.join("data/settings")).unwrap();
    std::fs::write(data.join("data/settings/perception_expander.lua"),
        "return {is_enabled=true,line_thick=4,margin=0.1,line_color_intensity=0.3,shift_each_pages=100,page_counter=0}").unwrap();
    let plugin = data.join("data/plugins/footerprobe.koplugin");
    std::fs::create_dir_all(&plugin).unwrap();
    std::fs::write(plugin.join("_meta.lua"), "return {fullname='Footer probe'}").unwrap();
    std::fs::write(
        plugin.join("main.lua"),
        r#"
        local P = require('ui/widget/widget'):extend{name='footerprobe',is_doc_only=true}
        function P:init()
            self.ui.view.footer:addAdditionalFooterContent(function() return 'Plugin footer' end)
        end
        return P
    "#,
    )
    .unwrap();
    let mut app = make_app(&data, vec![books]);
    app.start().unwrap();
    wait_for(&mut app, "scan", |a| !a.scan_running());
    enable(&mut app, &["perceptionexpander", "footerprobe"]);
    let id = book_id(&app, "2col.pdf");
    app.command(&format!("open_book:{id}"), &[]);
    wait_reader(&mut app);
    app.process(vec![]).unwrap();
    let with_plugins = app.frame().clone();
    kaia_gfx::golden::assert_matches_golden(&with_plugins, &golden("plugin_overlay.png"), 14);
    app.command("settings:set:plugins.enabled", &[Value::from(false)]);
    app.process(vec![]).unwrap();
    assert_ne!(with_plugins.get(128, 200), app.frame().get(128, 200));
    let start = with_plugins.stride() * (with_plugins.height() as usize - 45);
    assert_ne!(&with_plugins.data()[start..], &app.frame().data()[start..]);
    app.shutdown();
}

#[test]
fn plugin_dialog_error_disables_only_its_owner() {
    let _lock = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let data = scratch("plugin-dialog-error");
    let plugin = data.join("data/plugins/badtap.koplugin");
    std::fs::create_dir_all(&plugin).unwrap();
    std::fs::write(plugin.join("_meta.lua"), "return {fullname='Bad tap'}").unwrap();
    std::fs::write(
        plugin.join("main.lua"),
        r#"
        local P = require('ui/widget/widget'):extend{name='badtap'}
        function P:init() self.ui.menu:registerToMainMenu(self) end
        function P:addToMainMenu(items)
            items.bad_tap = {text='Open test dialog', callback=function()
                require('ui/uimanager'):show(require('ui/widget/infomessage'):new{
                    text='Tap to fail', onTapClose=function() error('dialog tap failed') end,
                })
            end}
        end
        return P
    "#,
    )
    .unwrap();
    let mut app = make_app(&data, vec![]);
    app.start().unwrap();
    enable(&mut app, &["badtap", "hello"]);
    app.command("plugins:menu:bad_tap", &[]);
    app.process(vec![]).unwrap();
    app.tap(kaia_gfx::Point::new(632, 840)).unwrap();
    app.process(vec![]).unwrap();
    assert!(app
        .services()
        .plugins
        .entries
        .iter()
        .any(|p| p.name == "badtap" && p.state.contains("dialog tap failed")));
    assert_eq!(app.services().plugins.foreign_window_count(), 0);
    app.command("settings:set:plugins.on.hello", &[Value::from(false)]);
    assert!(app
        .services()
        .plugins
        .entries
        .iter()
        .any(|p| p.name == "badtap" && p.state.contains("dialog tap failed")));
    app.command("settings:set:plugins.on.hello", &[Value::from(true)]);
    app.command("plugins:menu:hello_world", &[]);
    app.process(vec![]).unwrap();
    assert_eq!(app.services().plugins.foreign_window_count(), 1);
    app.shutdown();
}

#[test]
fn anki_dictionary_button_opens_its_profile_prompt() {
    let _lock = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let data = scratch("plugin-anki");
    let books = library_with_books(&data);
    copy_tree(
        &repo_root().join("rust/kaia-lua/tests/community/anki.koplugin"),
        &data.join("data/plugins/anki.koplugin"),
    );
    let mut app = make_app(&data, vec![books]);
    app.start().unwrap();
    wait_for(&mut app, "scan", |a| !a.scan_running());
    enable(&mut app, &["anki"]);
    let id = book_id(&app, "juliet.epub");
    app.command(&format!("open_book:{id}"), &[]);
    wait_reader(&mut app);
    assert!(
        app.services()
            .plugins
            .entries
            .iter()
            .any(|p| p.name == "anki" && p.state == "Works"),
        "{:?}",
        app.services().plugins.entries
    );
    app.command("reader:dictionary-word", &[Value::from("reader")]);
    app.process(vec![]).unwrap();
    tap_key(&mut app, "dictionary-plugins");
    assert_eq!(app.services().plugins.foreign_window_count(), 1);
    app.tap(kaia_gfx::Point::new(632, 145)).unwrap();
    app.process(vec![]).unwrap();
    assert!(
        app.services()
            .plugins
            .entries
            .iter()
            .any(|p| p.name == "anki" && p.state == "Works"),
        "{:?}",
        app.services().plugins.entries
    );
    assert_eq!(app.services().plugins.foreign_window_count(), 2);
    app.shutdown();
}

#[test]
fn bundled_defaults_open_an_epub_together() {
    let _lock = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let data = scratch("plugin-defaults");
    let books = library_with_books(&data);
    let mut app = make_app(&data, vec![books]);
    app.start().unwrap();
    wait_for(&mut app, "scan", |a| !a.scan_running());
    app.command("settings:set:plugins.enabled", &[Value::from(true)]);
    let id = book_id(&app, "juliet.epub");
    app.command(&format!("open_book:{id}"), &[]);
    wait_reader(&mut app);
    app.key(Key::RPgFwd).unwrap();
    wait_reader(&mut app);
    assert!(
        app.services().plugins.error().is_none(),
        "{:?}",
        app.services().plugins.error()
    );
    assert!(
        app.services()
            .plugins
            .entries
            .iter()
            .all(|p| !p.state.starts_with("Failed")),
        "{:?}",
        app.services().plugins.entries
    );
    app.command("settings:set:plugins.enabled", &[Value::from(false)]);
    assert!(!app.services().plugins.running());
    assert!(!app.services().plugins.idle_paused());
    app.shutdown();
}

#[test]
fn returning_to_an_open_reader_reconnects_its_plugin_host() {
    let _lock = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let data = scratch("plugin-readers");
    let books = library_with_books(&data);
    let mut app = make_app(&data, vec![books]);
    app.start().unwrap();
    wait_for(&mut app, "scan", |a| !a.scan_running());
    enable(&mut app, &["statistics"]);
    let pdf = book_id(&app, "2col.pdf");
    let epub = book_id(&app, "juliet.epub");
    app.command(&format!("open_book:{pdf}"), &[]);
    wait_reader(&mut app);
    app.key(Key::RPgFwd).unwrap();
    wait_reader(&mut app);
    let page = app
        .services()
        .plugins
        .plugin_value("statistics", "curr_page")
        .unwrap();
    app.command(&format!("open_book:{epub}"), &[]);
    wait_reader(&mut app);
    app.command("back", &[]);
    wait_reader(&mut app);
    assert_eq!(
        app.services()
            .plugins
            .plugin_value("statistics", "curr_page")
            .unwrap(),
        page
    );
    app.key(Key::RPgBack).unwrap();
    wait_reader(&mut app);
    assert_eq!(
        app.services()
            .plugins
            .plugin_value("statistics", "curr_page")
            .unwrap()
            .as_i64(),
        Some(page.as_i64().unwrap() - 1)
    );
    app.shutdown();
}

#[test]
fn anki_native_selection_saves_one_offline_card_across_restart() {
    use kaia_display::InputEvent;
    use kaia_gfx::Point;
    use kaia_input::{Phase, TouchEvent};
    use std::fs;
    use std::time::Duration;
    let _lock = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let data = scratch("plugin-anki-offline");
    let books = library_with_books(&data);
    let plugin = data.join("data/plugins/anki.koplugin");
    copy_tree(
        &repo_root().join("rust/kaia-lua/tests/community/anki.koplugin"),
        &plugin,
    );
    fs::write(plugin.join("profiles/default.lua"), r#"return {
        deckName='Reading', modelName='Basic', word_field='Word', def_field='Definition', context_field='Context'
    }"#).unwrap();
    fs::create_dir_all(data.join("data/settings")).unwrap();
    fs::write(
        data.join("data/settings/ankiconnect.lua"),
        "return {url='http://example.invalid'}",
    )
    .unwrap();
    fs::write(
        data.join("data/settings/anki_profiles.lua"),
        "return {['juliet.epub']='default'}",
    )
    .unwrap();
    let guard = data.join("data/plugins/offlinecheck.koplugin");
    fs::create_dir_all(&guard).unwrap();
    fs::write(guard.join("_meta.lua"), "return {fullname='Offline check'}").unwrap();
    fs::write(
        guard.join("main.lua"),
        r#"
        local P = require('ui/widget/container/widgetcontainer'):extend{attempts=0}
        function P:init()
            local function denied()
                self.attempts = self.attempts + 1
                error('network request in offline workflow')
            end
            local socket = require('socket')
            socket.tcp, socket.udp, socket.connect = denied, denied, denied
            require('socket.http').request = denied
            require('ssl.https').request = denied
        end
        return P
    "#,
    )
    .unwrap();
    let mut app = make_app(&data, vec![books.clone()]);
    app.start().unwrap();
    wait_for(&mut app, "scan", |a| !a.scan_running());
    enable(&mut app, &["anki", "offlinecheck"]);
    let id = book_id(&app, "juliet.epub");
    app.command(&format!("open_book:{id}"), &[]);
    wait_reader(&mut app);
    for _ in 0..8 {
        app.key(Key::RPgFwd).unwrap();
        wait_reader(&mut app);
    }
    app.process(vec![InputEvent::Touch(TouchEvent {
        slot: 0,
        phase: Phase::Down,
        pos: Point::new(200, 120),
        time_us: 1,
    })])
    .unwrap();
    std::thread::sleep(Duration::from_millis(700));
    app.iterate(Some(Duration::from_millis(1))).unwrap();
    app.process(vec![InputEvent::Touch(TouchEvent {
        slot: 0,
        phase: Phase::Up,
        pos: Point::new(200, 120),
        time_us: 2_000_000,
    })])
    .unwrap();
    wait_reader(&mut app);
    let mut selected = String::new();
    app.update_top::<kaia_ui::components::Label, _>("sel-word", |l, _| selected = l.text().into());
    let word = selected
        .split_whitespace()
        .next()
        .unwrap_or("")
        .trim_matches(|c: char| !c.is_alphanumeric())
        .to_string();
    assert!(!word.is_empty(), "selection label: {selected}");
    let dict = data.join("data/dict");
    fs::create_dir_all(&dict).unwrap();
    let definition = b"An offline test definition.";
    let mut index = word.as_bytes().to_vec();
    index.push(0);
    index.extend_from_slice(&0u32.to_be_bytes());
    index.extend_from_slice(&(definition.len() as u32).to_be_bytes());
    fs::write(dict.join("test.dict"), definition).unwrap();
    fs::write(dict.join("test.idx"), &index).unwrap();
    fs::write(dict.join("test.ifo"), format!("StarDict's dict ifo file\nversion=2.4.2\nbookname=Offline Dictionary\nwordcount=1\nidxfilesize={}\nsametypesequence=m\n", index.len())).unwrap();
    // The hold already looked up the word before this fixture was installed.
    app.services_mut().dictionary = None;
    tap_key(&mut app, "sel-dict");
    tap_key(&mut app, "dictionary-plugins");
    app.tap(Point::new(632, 145)).unwrap();
    app.process(vec![]).unwrap();
    assert!(
        app.services()
            .plugins
            .entries
            .iter()
            .all(|p| !p.enabled || p.state == "Works"),
        "{:?}",
        app.services().plugins.entries
    );
    let queue = data.join("data/settings/anki.koplugin_notes.json");
    let saved = fs::read_to_string(&queue).unwrap();
    assert_eq!(saved.lines().count(), 1);
    let card: serde_json::Value = serde_json::from_str(saved.trim()).unwrap();
    assert_eq!(card["data"]["fields"]["Word"], word);
    assert!(card["data"]["fields"]["Definition"]
        .as_str()
        .unwrap()
        .contains("An offline test definition."));
    let context = card["data"]["fields"]["Context"].as_str().unwrap();
    assert!(
        context.contains(&word) && context.len() > word.len() + 10,
        "{context}"
    );
    assert_eq!(
        app.services()
            .plugins
            .plugin_value("offlinecheck", "attempts")
            .unwrap(),
        Value::from(0i64)
    );
    app.shutdown();
    drop(app);
    let mut app = make_app(&data, vec![books]);
    app.start().unwrap();
    wait_for(&mut app, "scan", |a| !a.scan_running());
    app.command(&format!("open_book:{id}"), &[]);
    wait_reader(&mut app);
    assert_eq!(fs::read_to_string(queue).unwrap(), saved);
    assert!(app
        .services()
        .plugins
        .entries
        .iter()
        .any(|p| p.name == "anki" && p.state == "Works"));
    assert_eq!(
        app.services()
            .plugins
            .plugin_value("offlinecheck", "attempts")
            .unwrap(),
        Value::from(0i64)
    );
    app.shutdown();
}

#[test]
fn pencil_native_ink_draws_erases_bookmarks_and_reopens() {
    use kaia_display::InputEvent;
    use kaia_gfx::Point;
    use kaia_input::{PenEvent, Phase};
    use kaia_storage::Store;
    use std::time::Duration;
    let _lock = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let data = scratch("plugin-pencil-ink");
    let books = library_with_books(&data);
    copy_tree(
        &repo_root().join("rust/kaia-lua/tests/community/pencil.koplugin"),
        &data.join("data/plugins/pencil.koplugin"),
    );
    let mut app = make_app(&data, vec![books.clone()]);
    app.start().unwrap();
    wait_for(&mut app, "scan", |a| !a.scan_running());
    enable(&mut app, &["pencil"]);
    let id = book_id(&app, "juliet.epub");
    app.command(&format!("open_book:{id}"), &[]);
    wait_reader(&mut app);
    for _ in 0..8 {
        app.key(Key::RPgFwd).unwrap();
        wait_reader(&mut app);
    }
    assert!(
        app.services()
            .plugins
            .entries
            .iter()
            .any(|p| p.name == "pencil" && p.state == "Works"),
        "{:?}",
        app.services().plugins.entries
    );
    app.command("plugins:menu:pencil_annotation", &[]);
    app.process(vec![]).unwrap();
    app.tap(Point::new(632, 145)).unwrap();
    std::thread::sleep(Duration::from_millis(1100));
    app.iterate(Some(Duration::from_millis(1))).unwrap();
    app.tap(Point::new(1218, 48)).unwrap();
    app.process(vec![]).unwrap();
    assert_eq!(app.services().plugins.foreign_window_count(), 0);
    assert!(
        app.find_rect("ink-width").is_some(),
        "{:?}",
        app.services().plugins.entries
    );
    let pen = |app: &mut kaia_app::App<kaia_app::HeadlessBackend>, phase, x, y, eraser| {
        app.process(vec![InputEvent::Pen(PenEvent {
            phase,
            pos: Point::new(x, y),
            pressure: 700,
            eraser,
            button: false,
            time_us: 1,
        })])
        .unwrap();
    };
    pen(&mut app, Phase::Down, 320, 600, false);
    pen(&mut app, Phase::Move, 900, 660, false);
    pen(&mut app, Phase::Up, 900, 660, false);
    let store = Store::open(&data.join("data/kaia")).unwrap();
    let counts = store.ink_counts().unwrap();
    assert_eq!(counts.len(), 1);
    assert_eq!(counts[0].2, 1);
    let doc = counts[0].0.partial_md5.clone();
    let strokes = store.ink(&doc).unwrap();
    assert!(strokes[0].locator.as_ref().is_some_and(|s| s.starts_with('/')));
    assert_eq!(strokes[0].points[0].pressure, 700);
    let pixel = app.frame().get(600, 629);
    assert!(
        pixel.unwrap().luma() < 30,
        "native coordinates must match the pen input"
    );
    let state_path = data.join("data/kaia/books").join(format!("{doc}.json"));
    let saved = kaia_events::Settings::read_exact(&state_path).unwrap().unwrap();
    let annotations = saved.read("annotations");
    assert_eq!(annotations.array_len(), 1);
    let bookmark = annotations
        .as_table()
        .unwrap()
        .values()
        .next()
        .unwrap()
        .get("kaia_id")
        .clone();
    // Suspend drops an unfinished stroke, retaining the completed stroke.
    pen(&mut app, Phase::Down, 400, 700, false);
    pen(&mut app, Phase::Move, 500, 750, false);
    app.key(Key::Power).unwrap();
    assert!(app.sleeping());
    app.key(Key::Power).unwrap();
    assert!(!app.sleeping());
    wait_reader(&mut app);
    pen(&mut app, Phase::Up, 500, 750, false);
    assert_eq!(store.ink(&doc).unwrap(), strokes);
    // Disabling the extension relinquishes its native drawing mode.
    app.command("settings:set:plugins.on.pencil", &[Value::Bool(false)]);
    wait_reader(&mut app);
    app.process(vec![]).unwrap();
    pen(&mut app, Phase::Down, 400, 700, false);
    pen(&mut app, Phase::Up, 500, 750, false);
    assert_eq!(store.ink(&doc).unwrap(), strokes);
    app.command("settings:set:plugins.on.pencil", &[Value::Bool(true)]);
    wait_reader(&mut app);
    app.process(vec![]).unwrap();
    app.shutdown();
    drop(app);
    let mut app = make_app(&data, vec![books]);
    app.start().unwrap();
    wait_for(&mut app, "scan", |a| !a.scan_running());
    app.command(&format!("open_book:{id}"), &[]);
    wait_reader(&mut app);
    assert_eq!(
        app.frame().get(600, 629),
        pixel,
        "stroke pixels survive reopening"
    );
    assert_eq!(store.ink(&doc).unwrap(), strokes);
    let saved = kaia_events::Settings::read_exact(&state_path).unwrap().unwrap();
    assert_eq!(
        saved
            .read("annotations")
            .as_table()
            .unwrap()
            .values()
            .next()
            .unwrap()
            .get("kaia_id"),
        &bookmark
    );
    assert!(
        app.find_rect("ink-width").is_some(),
        "Pencil drawing mode must reopen"
    );
    pen(&mut app, Phase::Down, 600, 500, true);
    pen(&mut app, Phase::Move, 600, 800, true);
    pen(&mut app, Phase::Up, 600, 800, true);
    assert!(store.ink(&doc).unwrap().is_empty());
    let saved = kaia_events::Settings::read_exact(&state_path).unwrap().unwrap();
    assert_eq!(saved.read("annotations").array_len(), 0);
    app.shutdown();
}

#[test]
fn footcream_native_worker_scan_and_tap() {
    use std::fs;
    let _lock = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let data = scratch("plugin-footcream");
    let books = library_with_books(&data);
    let original = fs::read(books.join("juliet.epub")).unwrap();
    copy_tree(
        &repo_root().join("rust/kaia-lua/tests/community/footcream.koplugin"),
        &data.join("data/plugins/footcream.koplugin"),
    );
    let probe = data.join("data/plugins/footprobe.koplugin");
    fs::create_dir_all(&probe).unwrap();
    fs::write(
        probe.join("_meta.lua"),
        "return {fullname='Footcream workflow probe'}",
    )
    .unwrap();
    fs::write(
        probe.join("main.lua"),
        r#"
        local P = require('ui/widget/container/widgetcontainer'):extend{is_doc_only=true}
        function P:init() self.ui.menu:registerToMainMenu(self) end
        function P:addToMainMenu(items)
            items.scan_footcream = {text='Scan fixture', callback=function()
                local p = KAIA_PLUGIN.instances.footcream
                p:_startScan(self.ui.document)
                self.matches = #p._all_matches
                self.page = self.ui.document:getPageFromXPointer(p._all_matches[1].start)
                self.ui.rolling:gotoPage(self.page)
            end}
            items.inspect_footcream = {text='Inspect conversion', callback=function()
                local b = KAIA_PLUGIN.instances.footcream._current_boxes[1].box
                self.x, self.y = b.x + b.w / 2, b.y + b.h / 2
            end}
        end
        return P
    "#,
    )
    .unwrap();
    let mut app = make_app(&data, vec![books.clone()]);
    app.start().unwrap();
    wait_for(&mut app, "scan", |a| !a.scan_running());
    enable(&mut app, &["footcream", "footprobe"]);
    let id = book_id(&app, "juliet.epub");
    app.command(&format!("open_book:{id}"), &[]);
    wait_reader(&mut app);
    app.command("plugins:menu:scan_footcream", &[]);
    wait_reader(&mut app);
    app.process(vec![]).unwrap();
    assert!(
        app.services()
            .plugins
            .entries
            .iter()
            .all(|p| !p.enabled || p.state == "Works"),
        "{:?}",
        app.services().plugins.entries
    );
    assert!(
        app.services()
            .plugins
            .plugin_value("footprobe", "matches")
            .unwrap()
            .as_i64()
            .unwrap()
            > 0
    );
    assert!(
        app.services()
            .plugins
            .plugin_value("footprobe", "page")
            .unwrap()
            .as_i64()
            .unwrap()
            > 1
    );
    app.key(Key::Power).unwrap();
    assert!(app.sleeping());
    app.key(Key::Power).unwrap();
    assert!(!app.sleeping());
    wait_reader(&mut app);
    app.command("settings:set:plugins.on.footcream", &[Value::Bool(true)]);
    wait_reader(&mut app);
    app.process(vec![]).unwrap();
    assert!(
        app.services()
            .plugins
            .entries
            .iter()
            .all(|p| !p.enabled || p.state == "Works"),
        "{:?}",
        app.services().plugins.entries
    );
    // The pinned scan notice expires before the page receives a tap.
    std::thread::sleep(std::time::Duration::from_secs(4));
    app.iterate(Some(std::time::Duration::from_millis(1))).unwrap();
    app.command("plugins:menu:inspect_footcream", &[]);
    app.process(vec![]).unwrap();
    let x = app
        .services()
        .plugins
        .plugin_value("footprobe", "x")
        .unwrap()
        .as_f64()
        .unwrap() as i32;
    let y = app
        .services()
        .plugins
        .plugin_value("footprobe", "y")
        .unwrap()
        .as_f64()
        .unwrap() as i32;
    let before = app.services().plugins.foreign_window_count();
    app.tap(kaia_gfx::Point::new(x, y)).unwrap();
    assert!(app.services().plugins.foreign_window_count() > before);
    assert_eq!(fs::read(books.join("juliet.epub")).unwrap(), original);
    assert!(!books.join("juliet.sdr").exists());
    app.shutdown();
}
