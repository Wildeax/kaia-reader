--[[--
Paints a KOReader widget into a buffer that Rust owns.

The Rust side passes the address of a pixmap as a number. This module makes a
Blitbuffer over that memory for the duration of the call. The widget must not
keep the buffer after `paintTo` returns.
]]

local Blitbuffer = require("ffi/blitbuffer")

local M = {}

-- `bb_type` is a Blitbuffer type (TYPE_BB8 or TYPE_BBRGB24). `addr` is the
-- address of the first pixel. `stride` is the bytes per row.
function M.paint(widget, width, height, bb_type, addr, stride, x, y)
    local bb = Blitbuffer.new(width, height, bb_type, addr, stride)
    widget:paintTo(bb, x, y)
end

return M
