# Libarchive binding

This crate opens RAR and RAR5 archives through KOReader's shared libarchive.
The C declarations match the staged `archive.h` and `archive_entry.h`.
KOReader patches `archive_entry_filetype` to return `int` on every target.
The reader owns its handle, copies entry metadata before advancing, and closes
both the file and handle on drop. No archive member is extracted to disk.

## Build

The build script looks in `KAIA_ARCHIVE_LIB` first, then in
`KAIA_MUPDF_DEP_LIBS` and `KAIA_MUPDF_LIB`. Without overrides, a native build
uses the first sorted `base/build/*/staging/lib` directory for the host OS,
excluding Kobo builds. It requires `libarchive.dylib` on macOS or
`libarchive.so` on Linux. Cross builds must provide a target library directory.
There is no host system-library fallback.

`tools/kaia/build-rust-kobo.sh` sets `KAIA_ARCHIVE_LIB` to the staging library
directory for `arm-kobo-linux-gnueabihf` or `arm-kobov5-linux-gnueabihf`.
`tools/kaia/build-kobo.sh` builds libarchive with the rest of KOReader.
The Rust Docker image supplies the compiler, while the mounted target build
supplies the library. The Kobo package already includes `libs/libarchive.so.13`;
the package checks require it and the copied license summary.

## Limits and errors

Every entry must have a known size of at most 64 MiB. An archive may contain
at most 10,000 entries and 4 GiB of declared unpacked data. Reads reject data
that exceeds the declared size and EOF before that size. Skipped entries are
also read through these checks, including members before a page in a solid
archive. Errors permanently disable the reader.

Names must be UTF-8 and at most 4096 bytes. Absolute paths, drive-qualified
names, parent traversal, links, encrypted entries and special files are
rejected. Libarchive warnings and errors become Rust errors.

Libarchive 3.8.8 skips the payload CRC check at EOF for stored RAR4 entries.
Kaia still checks the returned length and image decoding errors. Header CRC
errors and reported decompression errors are rejected. The binding does not
claim independent payload-integrity verification.

`kaia-doc` indexes image names while it validates the archive. Opening reads
every entry once. The CBR backend then keeps the archive open at its position,
so the next page continues the stream. It caches the compressed bytes of recent
pages in at most 64 MiB, evicting the least recently used page first. A page
before the stream position reopens the archive and reads the entries before it.
A cold jump to a middle page of a solid archive therefore decodes every earlier
entry once. PNG/JPEG decoding uses `kaia-gfx`'s
4096-pixel side and 16-megapixel limits. Render buffers have a 16-megapixel
limit and a 16,384-pixel side limit.

## Fixtures and license

Tests generate minimal stored RAR4 records with PNG and JPEG pages. They also
decode the unchanged upstream compressed, solid and stored RAR5 fixtures
listed in `tests/fixtures/README.md`. The tests cover truncation, corrupt
headers, false sizes, unsafe paths, entry limits and render limits.

`COPYING.libarchive` is copied unchanged from the pinned libarchive 3.8.8
source archive. Its default license is BSD-2-Clause; the file also identifies
other licenses used by individual sources. The license inventory checks the
archive pin and the copied license's SHA-256. Kobo packaging includes this file.
Both target builds and on-device paging still need CI and device confirmation.
