# Libarchive test fixtures

These uuencoded fixtures and `../../COPYING.libarchive` come unchanged from
libarchive 3.8.8, the version pinned by `base/thirdparty/libarchive/CMakeLists.txt`.
The source archive is `libarchive-3.8.8.tar.xz`, MD5
`81a6d70daf20671ea2c5cd4bb1d371ae`. The fixture paths in that archive are
`libarchive/test/test_read_format_rar5_{stored,compressed,multiple_files_solid}.rar.uu`.

The tests decode them in memory, then read them through the staged libarchive.
The stored file contains `hello libarchive test suite!` followed by a newline.
The compressed file contains 1200 bytes; the solid archive has four 4096-byte files.
Tests also truncate the compressed fixture to check error handling.

`tests/common/mod.rs` writes minimal uncompressed RAR 4 records for generated
image, path, size, checksum and entry-count tests. It uses no RAR compressor.
