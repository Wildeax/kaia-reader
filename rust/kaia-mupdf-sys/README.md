# kaia-mupdf-sys

C shim and Rust bindings over KOReader's MuPDF build. The crate holds the only
`unsafe` code that talks to MuPDF. `kaia-doc` uses the safe `Document` type
from this crate.

## What is in the crate

- `csrc/kaia_mupdf.c` and `csrc/kaia_mupdf.h`: the shim. Each function wraps the
  MuPDF calls in `fz_try`/`fz_catch`, so a MuPDF exception never unwinds into
  Rust. The function returns a status code, and `kaia_doc_error()` returns the
  message.
- `src/ffi.rs`: the `extern "C"` declarations, written by hand to match the
  header.
- `src/lib.rs`: `Document`, which owns one fz_context and one document and frees
  both on drop.
- `build.rs`: compiles the shim with `cc` and links MuPDF and its dependencies.

Threading: a `Document` is not `Send` or `Sync`. Use one document from one
thread at a time. Each document has its own fz_context.

## Where the libraries come from

The build script looks for libraries in this order:

1. `KAIA_MUPDF_INCLUDE`, `KAIA_MUPDF_LIB` and `KAIA_MUPDF_DEP_LIBS`, when set.
2. The desktop KOReader build. The script globs `../../base/build/*/staging`,
   picks the directory whose name matches the host OS (`apple-darwin` or
   `linux`), skips Kobo directories, and uses the first match in sorted order.
   It prints a warning with the path it chose.

| Variable | Meaning | Default |
|---|---|---|
| `KAIA_MUPDF_INCLUDE` | Directory with `mupdf/fitz.h` | `<staging>/include` |
| `KAIA_MUPDF_LIB` | Directory with `libmupdf.a` and `libmupdf-third.a` | `<staging>/lib` |
| `KAIA_MUPDF_DEP_LIBS` | Path list of directories with the dependencies | the `KAIA_MUPDF_LIB` directory |
| `KAIA_MUPDF_FONTS` | Fallback font directory, used by tests | `<build>/fonts` |

The dependencies are freetype, harfbuzz, jpeg, webp, webpdemux, archive, z and
aes. A shared library (`.so` or `.dylib`) is linked when present. Otherwise the
static archive (`.a`) is linked. This matches KOReader's `libwrap-mupdf`, which
links the shared set.

The shim is compiled with the same defines KOReader passes to MuPDF:
`NOBUILTINFONT`, `FZ_ENABLE_HYPHEN=0`, `FZ_PLOTTERS_CMYK=0`, `HAVE_LIBAES=1`,
`HAVE_LIBARCHIVE=1` and `HAVE_WEBP=1`. Struct layouts depend on these.

## Fonts at run time

MuPDF loads fallback faces by name from the directory in the `FONTDIR`
environment variable, and defaults to `./fonts`. Layouts and some PDFs need
those faces. The app has to set `FONTDIR` before it opens a document.

A plain KOReader checkout downloads only three faces: `NimbusRomNo9L-Reg.cff`,
`NimbusRomNo9L-Med.cff` and `DroidSansMono.ttf`. MuPDF fails a whole page when
another requested face is missing, for example the italic in an EPUB. The tests
in `kaia-doc` therefore build a test-only directory under
`rust/target/kaia-doc-test/fonts` that links the missing names to the nearest
available face. The device package needs the full font set.

## Cross build for Kobo

KOReader's Kobo package ships `libs/libwrap-mupdf.so` and the shared
dependencies built for the device. The Kobo staging tree is
`base/build/arm-kobo-linux-gnueabihf/staging`. It lives in the Docker volume
used by `tools/kaia/build-kobo.sh`, so the cross build runs inside that
environment.

Point the build script at that tree and tell `cc` which compiler to use:

```sh
STAGING=/home/ko/build/base/build/arm-kobo-linux-gnueabihf/staging
export KAIA_MUPDF_INCLUDE=$STAGING/include
export KAIA_MUPDF_LIB=$STAGING/lib
export KAIA_MUPDF_DEP_LIBS=$STAGING/lib
export CC_armv7_unknown_linux_gnueabihf=arm-kobo-linux-gnueabihf-gcc
export AR_armv7_unknown_linux_gnueabihf=arm-kobo-linux-gnueabihf-ar
cargo build --target armv7-unknown-linux-gnueabihf -p kaia-doc
```

Notes for the cross build:

- The Kobo staging tree may not have every archive that the desktop tree has.
  The build script fails with a message that names the missing library.
- On a cross build the script does not add run-time search paths. The device
  finds the shared libraries in the package, so the package must ship
  `libfreetype`, `libharfbuzz`, `libjpeg`, `libwebp`, `libwebpdemux`,
  `libarchive` and `libz` next to the binary.
- The cross build was not run for this crate. The paths above are from the
  build script and the KOReader build layout.

## Tests

`cargo test -p kaia-mupdf-sys` covers the error paths that need no document:
a missing file, a path with a NUL byte and page numbers above `i32::MAX`. The
corpus tests in `kaia-doc/tests` cover the rest.
