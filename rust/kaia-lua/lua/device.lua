--[[--
Kaia shim for KOReader's device module.

KOReader picks a device implementation from the platform. The Kaia host has
one display, described by the global KAIA_DISPLAY that the Rust side sets
before any widget loads. This module extends KOReader's generic device (the
stubs and capability flags of `frontend/device/generic/device.lua`) and
replaces the parts that touch the hardware: the screen, the input and the
power code. `Device:init()` is not called, because it reads the reader settings
and sets up evdev input.

The screen methods copy `base/ffi/framebuffer.lua`. `scaleBySize` and
`scaleByDPI` use the same formulas, so a dialog gets the same sizes in Kaia as
in KOReader.
]]

-- luacheck: globals KAIA_DISPLAY KAIA_PATHS

local Blitbuffer = require("ffi/blitbuffer")
local Geom = require("ui/geometry")
local GenericDevice = require("device/generic/device")
local logger = require("logger")

local display = KAIA_DISPLAY
local BB_TYPE_BB8 = 1
local BB_TYPE_BBRGB32 = 5

-- KOReader's colour buffers are 32-bit. The Rust side paints colour frames
-- through a 32-bit copy (see `ui/foreign.rs`), so this type is the one widgets see.
local bb_type = display.color and BB_TYPE_BBRGB32 or BB_TYPE_BB8
local bytes_per_pixel = display.color and 4 or 1

local function yes() return true end
local function no() return false end

-- The screen object. It holds the same fields and methods as the framebuffer
-- object in `base/ffi/framebuffer.lua` that widgets use.
local Screen = {
    -- Set by the Rust side: the display size in pixels, and the DPI.
    dpi = display.dpi,
    dpi_override = display.dpi_override and true or nil,
    width = display.width,
    height = display.height,
    hw_dithering = false,
    sw_dithering = false,
    night_mode = false,
    wf_level = 0,
    wf_level_max = 0,
    swipe_animations = false,
    is_always_portrait = false,
    native_rotation_mode = 0,
    cur_rotation_mode = 0,
    blitbuffer_rotation_mode = 0,
    low_pan_rate = false,
}


-- The frame buffer that KOReader's widgets read the type from (`Screen.bb`,
-- for example to build a checkerboard for a missing image). The paint path
-- in Rust passes its own buffer to each widget, and this one is never painted.
-- `Screen.bb` is the frame buffer that KOReader's widgets read the type from,
-- and that `UIManager:widgetRepaint` paints into. On a grayscale panel it
-- wraps the frame that `LuaUi` owns. The frame stays at the same address while
-- the UI lives, so a widget that paints now draws into the frame, as in KOReader.
-- A colour frame is RGB24 and cannot be wrapped, so `Screen.bb` is a separate
-- 32-bit buffer there, and `widgetRepaint` does not reach the frame.
if display.frame_addr and not display.color then
    Screen.bb = Blitbuffer.new(Screen.width, Screen.height, bb_type, display.frame_addr, Screen.width)
else
    Screen.bb = Blitbuffer.new(Screen.width, Screen.height, bb_type)
end

function Screen:getRawSize()
    return { w = self.width, h = self.height }
end

function Screen:getWidth()
    return self.width
end

function Screen:getHeight()
    return self.height
end

function Screen:getSize()
    return Geom:new{ x = 0, y = 0, w = self.width, h = self.height }
end

function Screen:getScreenWidth()
    return self.width
end

function Screen:getScreenHeight()
    return self.height
end

function Screen:getDPI()
    return self.dpi
end

function Screen:setDPI(dpi)
    if not dpi then
        self.dpi = display.dpi
        self.dpi_override = nil
        return
    end
    self.dpi = dpi
    self.dpi_override = true
end

function Screen:clearDPI()
    self:setDPI(nil)
end

function Screen:scaleByDPI(dp)
    -- Round up, to make sure we never end up with a zero
    return math.ceil(dp * self:getDPI() / 160)
end

-- The same formula as `fb:scaleBySize` in `base/ffi/framebuffer.lua`.
function Screen:scaleBySize(px)
    local size_scale = math.min(self:getWidth(), self:getHeight()) / 600
    -- Without a DPI override, the DPI does not change the scale.
    local dpi_scale = size_scale
    if self.dpi_override then
        dpi_scale = self.dpi / 160
    end
    return math.ceil(px * (size_scale + dpi_scale) / 2)
end

function Screen:getRotationMode()
    return self.cur_rotation_mode
end

function Screen:getScreenMode()
    if self:getWidth() > self:getHeight() then
        return "landscape"
    end
    return "portrait"
end

function Screen:getScreenOrientation()
    return self:getScreenMode()
end

function Screen:getTouchRotation()
    return self.cur_rotation_mode
end

function Screen:setRotationMode()
    logger.warn("Kaia Lua host: screen rotation is not supported yet")
end

function Screen:isColorEnabled()
    return display.color
end

function Screen:isColorScreen()
    return display.color
end

function Screen:isBGRFrameBuffer()
    return false
end

-- The bytes per pixel of the frame buffer that paints use.
function Screen:getBytesPerPixel()
    return bytes_per_pixel
end

-- Refresh calls and paint brackets do nothing here. The Rust side queues the
-- refreshes that UIManager would have sent to the panel.
function Screen:beforePaint() end
function Screen:afterPaint() end
function Screen:refreshWaitForLast() end
function Screen:refreshFull() end
function Screen:refreshPartial() end
function Screen:refreshNoMergePartial() end
function Screen:refreshFlashPartial() end
function Screen:refreshUI() end
function Screen:refreshNoMergeUI() end
function Screen:refreshFlashUI() end
function Screen:refreshFast() end
function Screen:refreshA2() end
function Screen:close() end

-- Input groups, copied from `frontend/device/input.lua`. That module reads
-- evdev devices, so only its key group table is copied here.
local input = {
    group = {
        Cursor = { "Up", "Down", "Left", "Right" },
        PgFwd = { "RPgFwd", "LPgFwd" },
        PgBack = { "RPgBack", "LPgBack" },
        Back = { "Back" },
        Alphabet = {
            "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M",
            "N", "O", "P", "Q", "R", "S", "T", "U", "V", "W", "X", "Y", "Z",
        },
        AlphaNumeric = {
            "0", "1", "2", "3", "4", "5", "6", "7", "8", "9",
            "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M",
            "N", "O", "P", "Q", "R", "S", "T", "U", "V", "W", "X", "Y", "Z",
        },
        Numeric = {
            "0", "1", "2", "3", "4", "5", "6", "7", "8", "9",
        },
        Text = {
            " ", ".", "/",
            "0", "1", "2", "3", "4", "5", "6", "7", "8", "9",
            "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M",
            "N", "O", "P", "Q", "R", "S", "T", "U", "V", "W", "X", "Y", "Z",
        },
        Any = {
            " ", ".", "/",
            "0", "1", "2", "3", "4", "5", "6", "7", "8", "9",
            "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M",
            "N", "O", "P", "Q", "R", "S", "T", "U", "V", "W", "X", "Y", "Z",
            "Up", "Down", "Left", "Right", "Press", "Backspace", "End",
            "Back", "Sym", "AA", "Menu", "Home", "Del", "ScreenKB",
            "LPgBack", "RPgBack", "LPgFwd", "RPgFwd",
        },
    },
    disable_double_tap = true,
    tap_interval_override = nil,
    fake_event_args = {},
}

local Device = GenericDevice:extend{
    model = "Kaia",
    display_dpi = display.dpi,
    screen = Screen,
    input = input,
    -- Capabilities. The Libra Colour has touch, page-turn keys and an E Ink panel.
    isTouchDevice = display.touch and yes or no,
    hasMultitouch = false,
    hasKeys = yes,
    hasKeyboard = no,
    hasDPad = no,
    hasFewKeys = no,
    hasFrontlight = no,
    hasNaturalLight = no,
    hasNaturalLightMixer = no,
    hasEinkScreen = display.eink and yes or no,
    hasColorScreen = display.color and yes or no,
    hasWifiToggle = no,
    hasOTAUpdates = no,
    hasClipboard = yes,
    canSuspend = no,
    canStandby = no,
    canReboot = no,
    canPowerOff = no,
    canRestart = no,
    canSuspendOnPowerOff = no,
    isKobo = no,
    isKindle = no,
    isAndroid = no,
    isCervantes = no,
    isPocketBook = no,
    isRemarkable = no,
    isSonyPRSTUX = no,
    isSDL = no,
    isEmulator = no,
    isDesktop = yes,
    canHWDither = no,
    canHWInvert = no,
    hasBattery = no,
    canImportFiles = no,
    canOpenLink = no,
    canExternalDictLookup = no,
    canShareText = no,
    hasSystemFonts = no,
    hasOTARunning = no,
}

-- Kaia has no frontlight, power or Wi-Fi control in this layer, so these
-- answer with what the generic device gives.
function Device:init() end

function Device:getDeviceScreenDPI()
    return display.dpi
end

function Device:setScreenDPI(dpi)
    Screen:setDPI(dpi)
end

function Device:getPowerDevice()
    return nil
end

-- The font list reads "./fonts" from the working directory. The Kaia host
-- does not change the working directory, so the list reads the KOReader tree.
require("fontlist").fontdir = KAIA_PATHS.koreader .. "/fonts"

-- reader.lua does this once, and the font list and the document code read the
-- screen through it.
require("document/canvascontext"):init(Device)

return Device
