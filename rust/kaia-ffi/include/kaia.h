/*
 * kaia.h: C ABI of libkaia, version 1.
 *
 * Specification: docs/kaia/specs/ABI.md.
 * LuaJIT declarations: frontend/kaia/ffi/kaia_h.lua. A test in rust/kaia-ffi
 * checks that both files declare the same functions with the same signatures.
 *
 * Out buffers (kaia_buf *) must be zeroed or freed with kaia_buf_free before
 * the call. A call that succeeds overwrites the struct. A call that fails
 * leaves it untouched.
 */
#ifndef KAIA_H
#define KAIA_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Return codes. */
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

/* Bytes owned by libkaia. Free with kaia_buf_free. */
typedef struct kaia_buf {
    uint8_t *ptr;   /* owned by libkaia, NULL when len == 0 */
    size_t   len;
    size_t   cap;   /* private, pass back unchanged */
} kaia_buf;

/* Frees the bytes and zeroes the struct. Safe on a zeroed buffer and on a second call. */
void kaia_buf_free(kaia_buf *buf);

/* Version and error reporting. */
uint32_t kaia_abi_version(void);
int32_t kaia_version(kaia_buf *out);
int32_t kaia_last_error(kaia_buf *out);

/* Store handles. */
int32_t kaia_store_open(const char *dir, uint64_t *out_handle);
int32_t kaia_store_close(uint64_t handle);

/* Stickers. The JSON follows docs/kaia/specs/STORAGE.md. */
int32_t kaia_sticker_put(uint64_t h, const uint8_t *json, size_t len, kaia_buf *out_json);
int32_t kaia_sticker_get(uint64_t h, const char *id, kaia_buf *out_json);
int32_t kaia_sticker_list(uint64_t h, const char *partial_md5, kaia_buf *out_json_array);
int32_t kaia_sticker_list_all(uint64_t h, kaia_buf *out_json_array);
int32_t kaia_sticker_delete(uint64_t h, const char *id);
int32_t kaia_sticker_set_state(uint64_t h, const char *id, const char *state);

/* Assets. */
int32_t kaia_asset_add_file(uint64_t h, const char *path, const char *license, kaia_buf *out_json);
int32_t kaia_asset_path(uint64_t h, const char *sha256, kaia_buf *out_path);
int32_t kaia_store_gc(uint64_t h, uint32_t *out_removed);

/* Documents. */
int32_t kaia_document_seen(uint64_t h, const char *partial_md5, uint64_t file_size, const char *title, int32_t *out_changed);

/* Export and import. */
int32_t kaia_export(uint64_t h, kaia_buf *out_dir);
int32_t kaia_import(uint64_t h, const char *dir, kaia_buf *out_report_json);

/* Re-anchoring. Pure, needs no store. */
int32_t kaia_quote_locate(const char *haystack, const char *exact, const char *prefix, const char *suffix, int64_t *out_char_index);

#ifdef __cplusplus
}
#endif

#endif /* KAIA_H */
