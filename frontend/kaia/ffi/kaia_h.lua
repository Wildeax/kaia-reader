--[[--
LuaJIT declarations for the C ABI in rust/kaia-ffi/include/kaia.h.

This file repeats the header without preprocessor lines and comments. Error
code names live in kaia/rustlib. A Rust test (rust/kaia-ffi/tests/header.rs)
checks that every function here has the same signature as in kaia.h.

@module kaia.ffi.kaia_h
--]]

local ffi = require("ffi")

ffi.cdef[[
typedef struct kaia_buf {
    uint8_t *ptr;
    size_t   len;
    size_t   cap;
} kaia_buf;
void kaia_buf_free(kaia_buf *buf);
uint32_t kaia_abi_version(void);
int32_t kaia_version(kaia_buf *out);
int32_t kaia_last_error(kaia_buf *out);
int32_t kaia_store_open(const char *dir, uint64_t *out_handle);
int32_t kaia_store_close(uint64_t handle);
int32_t kaia_sticker_put(uint64_t h, const uint8_t *json, size_t len, kaia_buf *out_json);
int32_t kaia_sticker_get(uint64_t h, const char *id, kaia_buf *out_json);
int32_t kaia_sticker_list(uint64_t h, const char *partial_md5, kaia_buf *out_json_array);
int32_t kaia_sticker_list_all(uint64_t h, kaia_buf *out_json_array);
int32_t kaia_sticker_delete(uint64_t h, const char *id);
int32_t kaia_sticker_set_state(uint64_t h, const char *id, const char *state);
int32_t kaia_asset_add_file(uint64_t h, const char *path, const char *license, kaia_buf *out_json);
int32_t kaia_asset_path(uint64_t h, const char *sha256, kaia_buf *out_path);
int32_t kaia_store_gc(uint64_t h, uint32_t *out_removed);
int32_t kaia_document_seen(uint64_t h, const char *partial_md5, uint64_t file_size, const char *title, int32_t *out_changed);
int32_t kaia_export(uint64_t h, kaia_buf *out_dir);
int32_t kaia_import(uint64_t h, const char *dir, kaia_buf *out_report_json);
int32_t kaia_quote_locate(const char *haystack, const char *exact, const char *prefix, const char *suffix, int64_t *out_char_index);
]]
