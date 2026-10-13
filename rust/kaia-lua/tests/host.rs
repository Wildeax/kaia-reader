//! Startup of the host: the LuaJIT runtime, the module paths and the globals.

mod common;

use common::{host, koreader_dir, serial, shim_dir, TempDir};
use kaia_lua::{Error, LuaHost, LuaPaths, LUAJIT_VERSION};

#[test]
fn runtime_is_koreader_luajit() {
    let _guard = serial();
    let data = TempDir::new("runtime");
    let host = host(&data);
    let version = host
        .eval("version.lua", "return jit.version")
        .expect("jit.version");
    assert_eq!(version.as_str(), Some(LUAJIT_VERSION));
    assert!(LUAJIT_VERSION.starts_with("LuaJIT 2.1."), "{LUAJIT_VERSION}");
    // ffi and debug are loaded, as KOReader's luajit executable loads them.
    assert_eq!(
        host.eval(
            "libs.lua",
            "return type(ffi.load) .. ' ' .. type(debug.traceback)"
        )
        .unwrap()
        .as_str(),
        Some("function function")
    );
}

#[test]
fn shim_directory_is_searched_first() {
    let _guard = serial();
    let data = TempDir::new("paths");
    let host = host(&data);
    let shim = shim_dir().join("datastorage.lua");
    let found = host
        .eval(
            "search.lua",
            "return package.searchpath('datastorage', package.path)",
        )
        .unwrap();
    assert_eq!(found.as_str(), Some(shim.to_str().unwrap()));
    let util = host
        .eval("search.lua", "return package.searchpath('util', package.path)")
        .unwrap();
    let util_path = koreader_dir().join("frontend/util.lua");
    assert_eq!(util.as_str(), Some(util_path.to_str().unwrap()));
    let lfs = host
        .eval(
            "search.lua",
            "return package.searchpath('libs/libkoreader-lfs', package.cpath)",
        )
        .unwrap();
    let lfs_path = koreader_dir().join("libs/libkoreader-lfs.so");
    assert_eq!(lfs.as_str(), Some(lfs_path.to_str().unwrap()));
}

#[test]
fn globals_match_reader_startup() {
    let _guard = serial();
    let data = TempDir::new("globals");
    let host = host(&data);
    host.exec(
        "globals.lua",
        "assert(G_defaults:readDefaultSetting('DHINTCOUNT') == 1)
         assert(G_defaults.file == KAIA_PATHS.data .. '/defaults.custom.lua')
         assert(G_reader_settings.file == KAIA_PATHS.data .. '/settings.reader.lua')
         assert(KAIA_PATHS.libs == KAIA_PATHS.koreader .. '/libs')",
    )
    .expect("globals");
    assert!(data.path().is_dir());
    // The reader settings file is created on flush, in the data folder.
    host.exec(
        "flush.lua",
        "G_reader_settings:saveSetting('kaia_probe', 1); G_reader_settings:flush()",
    )
    .unwrap();
    assert!(data.path().join("settings.reader.lua").is_file());
}

#[test]
fn startup_does_not_load_ui_modules() {
    let _guard = serial();
    let data = TempDir::new("no-ui");
    let host = host(&data);
    let loaded = host
        .eval(
            "no-ui.lua",
            "local ui = {'device', 'ui/uimanager', 'document/canvascontext', 'ui/widget/infomessage'}
             local out = {}
             for _, name in ipairs(ui) do out[#out + 1] = name .. '=' .. tostring(package.loaded[name] ~= nil) end
             return table.concat(out, ' ')",
        )
        .unwrap();
    assert_eq!(
        loaded.as_str(),
        Some("device=false ui/uimanager=false document/canvascontext=false ui/widget/infomessage=false")
    );
}

#[test]
fn native_libraries_resolve_in_the_koreader_tree() {
    let _guard = serial();
    let data = TempDir::new("native");
    let host = host(&data);
    // The test binary runs from the crate directory, not from the KOReader tree.
    let cwd = std::env::current_dir().unwrap();
    assert_ne!(cwd, koreader_dir());
    host.exec(
        "native.lua",
        "local lfs = ffi.load('libs/libkoreader-lfs.so')
         assert(lfs ~= nil)
         local utf8proc = ffi.loadlib('utf8proc', '3')
         assert(utf8proc ~= nil)
         assert(KAIA_PATHS.libs == KAIA_PATHS.koreader .. '/libs')",
    )
    .expect("native libraries load from the KOReader tree");
}

#[test]
fn missing_koreader_tree_is_an_init_error() {
    let _guard = serial();
    let data = TempDir::new("missing");
    let paths = LuaPaths::new(shim_dir(), data.path().join("no-such-tree"), data.path());
    match LuaHost::new(paths) {
        Err(Error::Init(message)) => assert!(message.contains("setupkoenv.lua"), "{message}"),
        Err(other) => panic!("expected an init error, got {other}"),
        Ok(_) => panic!("expected an init error"),
    }
}

#[test]
fn data_folder_is_created() {
    let _guard = serial();
    let data = TempDir::new("create");
    let nested = data.path().join("kaia/data");
    let host = LuaHost::new(LuaPaths::new(shim_dir(), koreader_dir(), &nested)).expect("host");
    assert!(nested.is_dir());
    assert_eq!(host.paths().data_dir, nested);
}

/// KOReader's native modules bind to the dynamic libluajit. Linking the same
/// dynamic library keeps one LuaJIT image in the process. A static copy would
/// add a second one when `libs/libkoreader-lfs.so` loads.
#[cfg(target_os = "macos")]
#[test]
fn one_luajit_image_in_the_process() {
    let _guard = serial();
    let data = TempDir::new("images");
    let host = host(&data);
    let count = host
        .eval(
            "images.lua",
            "local ffi = require('ffi')
             ffi.cdef[[
                 uint32_t _dyld_image_count(void);
                 const char *_dyld_get_image_name(uint32_t image_index);
             ]]
             require('libs/libkoreader-lfs')
             local n = 0
             for i = 0, tonumber(ffi.C._dyld_image_count()) - 1 do
                 if ffi.string(ffi.C._dyld_get_image_name(i)):find('libluajit', 1, true) then
                     n = n + 1
                 end
             end
             return n",
        )
        .unwrap();
    assert_eq!(count.as_i64(), Some(1));
}
