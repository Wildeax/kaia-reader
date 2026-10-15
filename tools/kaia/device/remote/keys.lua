-- Presses Kobo hardware keys on the gpio-keys device (/dev/input/event0).
-- Usage: luajit kobo-keys.lua both | fwd | back
local ffi = require("ffi")
ffi.cdef[[
struct timeval_k { long tv_sec; long tv_usec; };
struct input_event_k { struct timeval_k time; uint16_t type; uint16_t code; int32_t value; };
int open(const char *path, int flags);
int close(int fd);
long write(int fd, const void *buf, unsigned long n);
int usleep(unsigned int usec);
]]
local C = ffi.C
local fd = C.open("/dev/input/event0", 2)
assert(fd >= 0, "cannot open /dev/input/event0")
local ev = ffi.new("struct input_event_k")
local function emit(t, c, v)
    ev.type, ev.code, ev.value = t, c, v
    assert(C.write(fd, ev, ffi.sizeof(ev)) == ffi.sizeof(ev))
end
local keys = ({ both = { 193, 194 }, back = { 193 }, fwd = { 194 } })[arg[1]]
assert(keys, "usage: both | fwd | back")
for _, k in ipairs(keys) do emit(1, k, 1) end
emit(0, 0, 0)
C.usleep(300000)
for _, k in ipairs(keys) do emit(1, k, 0) end
emit(0, 0, 0)
C.close(fd)
