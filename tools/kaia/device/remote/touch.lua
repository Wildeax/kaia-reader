-- Sends synthetic finger touches to the Kobo Libra Colour touchscreen.
-- Usage: luajit kobo-touch.lua info
--        luajit kobo-touch.lua tap X Y
--        luajit kobo-touch.lua hold X Y MS
--        luajit kobo-touch.lua swipe X1 Y1 X2 Y2 MS
--        luajit kobo-touch.lua pinch CX CY FROM TO MS   (two fingers on a vertical line, distance FROM to TO)
-- X and Y are portrait screen pixels (0..1263, 0..1679).
local ffi = require("ffi")
ffi.cdef[[
struct timeval_k { long tv_sec; long tv_usec; };
struct input_event_k { struct timeval_k time; uint16_t type; uint16_t code; int32_t value; };
struct input_absinfo_k { int32_t value, minimum, maximum, fuzz, flat, resolution; };
int open(const char *path, int flags);
int close(int fd);
long write(int fd, const void *buf, unsigned long n);
int ioctl(int fd, unsigned long req, ...);
int usleep(unsigned int usec);
]]
local C = ffi.C
local EV_SYN, EV_KEY, EV_ABS = 0, 1, 3
local BTN_TOUCH = 0x14a
local ABS_MT_SLOT, ABS_MT_TOUCH_MAJOR, ABS_MT_POSITION_X, ABS_MT_POSITION_Y = 0x2f, 0x30, 0x35, 0x36
local ABS_MT_TOOL_TYPE, ABS_MT_TRACKING_ID, ABS_MT_PRESSURE = 0x37, 0x39, 0x3a
local W, H = 1264, 1680
local fd = C.open("/dev/input/event1", 2)
assert(fd >= 0, "cannot open /dev/input/event1")
local function absinfo(code)
    local info = ffi.new("struct input_absinfo_k")
    assert(C.ioctl(fd, 0x80184540 + code, info) == 0)
    return info
end
local maxx, maxy = absinfo(ABS_MT_POSITION_X).maximum, absinfo(ABS_MT_POSITION_Y).maximum
-- KOReader's Kobo input swaps x and y, then mirrors y on this model. Invert that.
local function raw(x, y)
    local rx = math.floor((H - 1 - y) * maxx / (H - 1) + 0.5)
    local ry = math.floor(x * maxy / (W - 1) + 0.5)
    return rx, ry
end
local ev = ffi.new("struct input_event_k")
local function emit(t, c, v)
    ev.type, ev.code, ev.value = t, c, v
    assert(C.write(fd, ev, ffi.sizeof(ev)) == ffi.sizeof(ev))
end
local id = os.time() % 30000
local function down(x, y)
    local rx, ry = raw(x, y)
    emit(EV_ABS, ABS_MT_SLOT, 0)
    emit(EV_ABS, ABS_MT_TRACKING_ID, id)
    emit(EV_ABS, ABS_MT_TOOL_TYPE, 0)
    emit(EV_ABS, ABS_MT_POSITION_X, rx)
    emit(EV_ABS, ABS_MT_POSITION_Y, ry)
    emit(EV_ABS, ABS_MT_TOUCH_MAJOR, 10)
    emit(EV_ABS, ABS_MT_PRESSURE, 60)
    emit(EV_KEY, BTN_TOUCH, 1)
    emit(EV_SYN, 0, 0)
end
local function move(x, y)
    local rx, ry = raw(x, y)
    emit(EV_ABS, ABS_MT_POSITION_X, rx)
    emit(EV_ABS, ABS_MT_POSITION_Y, ry)
    emit(EV_SYN, 0, 0)
end
local function up()
    emit(EV_ABS, ABS_MT_TRACKING_ID, -1)
    emit(EV_KEY, BTN_TOUCH, 0)
    emit(EV_SYN, 0, 0)
end
local cmd = arg[1]
local n = {}
for i = 2, #arg do n[#n + 1] = tonumber(arg[i]) end
if cmd == "info" then
    print("maxx", maxx, "maxy", maxy)
elseif cmd == "tap" then
    down(n[1], n[2]); C.usleep(60000); up()
elseif cmd == "hold" then
    down(n[1], n[2]); C.usleep((n[3] or 1000) * 1000); up()
elseif cmd == "swipe" then
    local steps = 12
    down(n[1], n[2])
    for i = 1, steps do
        C.usleep(math.floor((n[5] or 300) * 1000 / steps))
        move(n[1] + (n[3] - n[1]) * i / steps, n[2] + (n[4] - n[2]) * i / steps)
    end
    up()
elseif cmd == "pinch" then
    local cx, cy, from, to, ms = n[1], n[2], n[3], n[4], n[5] or 500
    local function both(d, first)
        for slot = 0, 1 do
            local y = (slot == 0) and (cy - d / 2) or (cy + d / 2)
            local rx, ry = raw(cx, y)
            emit(EV_ABS, ABS_MT_SLOT, slot)
            if first then
                emit(EV_ABS, ABS_MT_TRACKING_ID, id + slot)
                emit(EV_ABS, ABS_MT_TOOL_TYPE, 0)
                emit(EV_ABS, ABS_MT_TOUCH_MAJOR, 10)
                emit(EV_ABS, ABS_MT_PRESSURE, 60)
            end
            emit(EV_ABS, ABS_MT_POSITION_X, rx)
            emit(EV_ABS, ABS_MT_POSITION_Y, ry)
        end
        if first then emit(EV_KEY, BTN_TOUCH, 1) end
        emit(EV_SYN, 0, 0)
    end
    both(from, true)
    local steps = 15
    for i = 1, steps do
        C.usleep(math.floor(ms * 1000 / steps))
        both(from + (to - from) * i / steps, false)
    end
    for slot = 0, 1 do
        emit(EV_ABS, ABS_MT_SLOT, slot)
        emit(EV_ABS, ABS_MT_TRACKING_ID, -1)
    end
    emit(EV_KEY, BTN_TOUCH, 0)
    emit(EV_SYN, 0, 0)
else
    io.stderr:write("usage: info | tap X Y | hold X Y MS | swipe X1 Y1 X2 Y2 MS\n")
    os.exit(2)
end
C.close(fd)
