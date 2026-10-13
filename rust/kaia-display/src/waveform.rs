//! E Ink refresh requests: which waveform to drive, and which region to update.

use kaia_gfx::Rect;

/// The E Ink waveform a refresh uses. Each variant names a driver mode on the Kobo
/// MTK panel. The table on [`Waveform`] shows which KOReader refresh mode maps to
/// which variant.
///
/// # KOReader modes
///
/// The UI crate turns KOReader's refresh modes (the `refreshtype` argument of
/// `UIManager:setDirty`) into a [`Refresh`] with this table. The rows follow
/// `base/ffi/framebuffer_mxcfb.lua` for the Kobo MTK driver, which the Libra Colour
/// uses. The method names are the `framebuffer:refresh*Imp` entry points in that
/// file (`base/ffi/framebuffer.lua` supplies the fallbacks for the `[partial]` and
/// `[ui]` rows).
///
/// | KOReader mode | Method | Waveform | Flash | Notes |
/// |---|---|---|---|---|
/// | `full` | `refreshFullImp` | `Gc16` | yes | With the dither flag on a color panel, the driver uses `Gcc16`. |
/// | `flashpartial` | `refreshFlashPartialImp` | `Glr16` | yes | `avoid_flashing_ui` turns it into `partial`. |
/// | `partial` | `refreshPartialImp` | `Glr16` | no | Every `FULL_REFRESH_COUNT`th partial is promoted: to `full` without a region, to `flashui` with one. |
/// | `[partial]` | `refreshNoMergePartialImp` | `Glr16` | no | Same waveform as `partial`. Only the merging of refreshes differs. |
/// | `flashui` | `refreshFlashUIImp` | `Auto` | yes | `avoid_flashing_ui` turns it into `ui`. |
/// | `ui` | `refreshUIImp` | `Auto` | no | Widget updates. |
/// | `[ui]` | `refreshNoMergeUIImp` | `Auto` | no | Same waveform as `ui`. Only the merging of refreshes differs. |
/// | `fast` | `refreshFastImp` | `Du` | no | Fast update, black and white. |
/// | `a2` | `refreshA2Imp` | `A2` | no | One bit per pixel. |
///
/// `Gl16` is not used by this table. It is listed for the non-REAGL Kindle path in
/// KOReader, where partial refreshes use GL16 fast.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Waveform {
    /// The driver chooses the waveform. KOReader's `AUTO`.
    Auto,
    /// Fast black and white. KOReader's `DU`.
    Du,
    /// Gray with 16 levels, without ghost reduction.
    Gl16,
    /// Full quality gray with 16 levels. Clears ghosting, usually with a flash.
    Gc16,
    /// One bit per pixel, the fastest mode. KOReader's `A2`.
    A2,
    /// Gray with 16 levels and ghost reduction. KOReader's `GLR16` (REAGL).
    Glr16,
    /// Full quality color. KOReader's `GCC16`, used for color content on color panels.
    Gcc16,
}

/// One request to update part of the screen.
///
/// `region` is in logical pixels, the orientation the frame is drawn in. It must lie
/// inside the screen (see [`crate::check_present`]). An empty region is accepted and
/// asks for nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Refresh {
    pub region: Rect,
    pub waveform: Waveform,
    /// A flash inverts the region once to clear ghosting. The UI sets it for full
    /// screen refreshes and flashing UI updates.
    pub flash: bool,
}

impl Refresh {
    pub const fn new(region: Rect, waveform: Waveform, flash: bool) -> Self {
        Self {
            region,
            waveform,
            flash,
        }
    }
}
