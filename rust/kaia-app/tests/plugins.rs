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
