use std::env;

fn main() {
    println!("cargo:rerun-if-env-changed=KAIA_SPIKE_LUAJIT_DIR");
    println!("cargo:rerun-if-changed=build.rs");

    // KOReader's libluajit.dylib has the install name @rpath/libluajit.dylib.
    // The host must find that same file at run time, so record its directory.
    if let Ok(dir) = env::var("KAIA_SPIKE_LUAJIT_DIR") {
        println!("cargo:rustc-link-arg-bins=-Wl,-rpath,{dir}");
    }
}
