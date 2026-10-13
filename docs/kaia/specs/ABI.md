# Kaia C ABI, version 1 (KR-008, KR-009)

Status: proposed. `libkaia` is a Rust `cdylib` (`libkaia.so` on Linux and Kobo, `libkaia.dylib` on macOS) loaded by the existing LuaJIT through `ffi.load`. Lua calls Rust and receives results. Rust never calls back into Lua in version 1.

The header `rust/kaia-ffi/include/kaia.h` is the single source of truth for the Lua `ffi.cdef` in `frontend/kaia/ffi/kaia_h.lua`. A test compares the two.

## Rules

- Only fixed-width integers, `double`, `const char*` (NUL-terminated UTF-8), `const uint8_t*` plus length, and the structs below cross the boundary.
- No Rust `String`, `Vec`, references, trait objects or enums with data cross the boundary.
- Every exported function wraps its body in `std::panic::catch_unwind`. A panic returns `KAIA_E_PANIC` and stores the panic message as the last error. The crate sets `panic = "unwind"` so `catch_unwind` works; no unwinding crosses `extern "C"`.
- Stores are referenced by a `uint64_t` handle, not a pointer. Handles come from a process-wide registry protected by a `Mutex`. A handle encodes a slot index and a generation, so a closed or never-issued handle returns `KAIA_E_BAD_HANDLE` and never dereferences freed memory.
- All functions are safe to call from one thread at a time per store. The registry lock serializes calls; LuaJIT calls from the single UI thread.
- Inputs that are `NULL`, not valid UTF-8, or longer than the documented limits return `KAIA_E_INVALID_ARG`.

## Owned buffers

```c
typedef struct kaia_buf {
    uint8_t *ptr;   /* owned by libkaia, NULL when len == 0 */
    size_t   len;
    size_t   cap;   /* private, pass back unchanged */
} kaia_buf;

void kaia_buf_free(kaia_buf *buf);  /* frees and zeroes; safe on a zeroed buf and on double free of the same zeroed struct */
```

Functions that return data write into a caller-provided `kaia_buf*`. Lua owns the struct (allocated with `ffi.new("kaia_buf[1]")`) and must call `kaia_buf_free`. The Lua wrapper does this with `ffi.gc` so a forgotten free is still collected.

## Error codes

```c
#define KAIA_OK                  0
#define KAIA_E_INVALID_ARG      -1
#define KAIA_E_BAD_HANDLE       -2
#define KAIA_E_NOT_FOUND        -3
#define KAIA_E_IO               -4
#define KAIA_E_DB               -5
#define KAIA_E_SCHEMA_TOO_NEW   -6
#define KAIA_E_VALIDATION       -7
#define KAIA_E_ASSET_REJECTED   -8
#define KAIA_E_PANIC            -99
```

`int32_t kaia_last_error(kaia_buf *out);` copies the message of the most recent error on the calling thread (UTF-8, no NUL). Returns `KAIA_OK`, or `KAIA_E_NOT_FOUND` if there is none.

## Functions

```c
uint32_t kaia_abi_version(void);                      /* returns 1 */
int32_t  kaia_version(kaia_buf *out);                  /* crate version string, e.g. "0.1.0" */

int32_t  kaia_store_open(const char *dir, uint64_t *out_handle);
int32_t  kaia_store_close(uint64_t handle);

/* Stickers. JSON follows docs/kaia/specs/STORAGE.md. */
int32_t  kaia_sticker_put(uint64_t h, const uint8_t *json, size_t len, kaia_buf *out_json);
int32_t  kaia_sticker_get(uint64_t h, const char *id, kaia_buf *out_json);
int32_t  kaia_sticker_list(uint64_t h, const char *partial_md5, kaia_buf *out_json_array);
int32_t  kaia_sticker_list_all(uint64_t h, kaia_buf *out_json_array);
int32_t  kaia_sticker_delete(uint64_t h, const char *id);
int32_t  kaia_sticker_set_state(uint64_t h, const char *id, const char *state);

/* Assets */
int32_t  kaia_asset_add_file(uint64_t h, const char *path, const char *license, kaia_buf *out_json);
int32_t  kaia_asset_path(uint64_t h, const char *sha256, kaia_buf *out_path);
int32_t  kaia_store_gc(uint64_t h, uint32_t *out_removed);

/* Documents */
int32_t  kaia_document_seen(uint64_t h, const char *partial_md5, uint64_t file_size, const char *title, int32_t *out_changed);

/* Export and import */
int32_t  kaia_export(uint64_t h, kaia_buf *out_dir);
int32_t  kaia_import(uint64_t h, const char *dir, kaia_buf *out_report_json);

/* Re-anchoring (pure, no store needed) */
int32_t  kaia_quote_locate(const char *haystack, const char *exact, const char *prefix, const char *suffix, int64_t *out_char_index);
```

### Semantics

- `kaia_sticker_put` inserts when the JSON has no `id`, otherwise updates. Returns the stored sticker JSON with `id`, `created_at` and `updated_at` filled. Validation errors return `KAIA_E_VALIDATION` with a message naming the field.
- `kaia_sticker_list` returns `[]` for an unknown document.
- `kaia_document_seen` records the document and sets `*out_changed = 1` when the stored `file_size` differs. In that case every `attached` sticker of the document moves to `needs_review` in the same transaction.
- `kaia_store_gc` removes assets that no sticker references, except an asset added in the last 60 minutes that no sticker has saved yet. The store keeps that time in its database (schema version 2), so the grace period survives a close and reopen.
- `kaia_quote_locate` returns the character index (Unicode scalar values, 0-based) of the start of `exact` in `haystack`, following the rules in STORAGE.md, or `-1` with `KAIA_OK` when there is no match.

## Lua wrapper contract

`frontend/kaia/rustlib.lua` loads the library and returns `nil` plus a reason when it is missing or `kaia_abi_version()` is not 1. Every Kaia feature that needs Rust checks for `nil` and stays disabled. With the library removed, KOReader behaves exactly as the baseline.

## Required tests (KR-009)

Rust side, in `rust/kaia-ffi/tests/`:

1. Ten thousand open, put, list, close cycles with no growth in live allocations (count with a test global allocator).
2. Closed handle, never-issued handle and handle `0` return `KAIA_E_BAD_HANDLE`.
3. `NULL` pointers and invalid UTF-8 return `KAIA_E_INVALID_ARG`.
4. A debug-only export `kaia_debug_panic` (behind the `test-panic` feature) returns `KAIA_E_PANIC` and the process survives.
5. A 64 MiB JSON input is rejected before parsing (limit 1 MiB).
6. `kaia_buf_free` on a zeroed buf and twice on the same buf is safe.
7. Property tests for `kaia_quote_locate` with random text.

Lua side, in `spec/unit/kaia_rustlib_spec.lua`: the same calls through LuaJIT FFI, plus the missing-library fallback.
