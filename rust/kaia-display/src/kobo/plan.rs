//! Turns a [`Refresh`] into the FBInk waveform, flash and wait calls for the Kobo
//! panel. This module has no device access, so the mapping runs on the host.
//!
//! The rules come from KOReader's `base/ffi/framebuffer_mxcfb.lua`, which drives the
//! MTK panel of the Libra Colour: `mxc_update` for the waits and the promotion to
//! color and FULL modes, and the MTK setup block for the mode assignments. Each rule
//! names the behavior it copies. Nothing here has been run on a device.

use kaia_fbink_sys as fb;
use kaia_gfx::{PixelFormat, Rect, Size};

use crate::waveform::{Refresh, Waveform};

/// Partial regions are widened to this grid before they reach the panel.
/// KOReader sets `alignment_constraint = 16` for MTK (`framebuffer_mxcfb.lua`, the
/// off-by-one note in the MTK block), because the driver cuts refreshes at exact
/// coordinates one pixel short.
pub const REGION_ALIGN: i32 = 16;

/// The FBInk calls for one refresh, in the order the backend makes them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RefreshPlan {
    /// FBInk waveform mode, one of the `fb::WFM_*` constants.
    pub waveform: u8,
    /// Ask for a flash. FBInk turns this into `UPDATE_MODE_FULL`.
    pub flashing: bool,
    /// Use the CFA post-process mode (color content on the color panel).
    pub color_post: bool,
    /// Wait for the previous update to finish, after the pixels are drawn and before
    /// the refresh is sent.
    pub wait_before: bool,
    /// Wait for this update to finish, right after it is sent.
    pub wait_after: bool,
    /// Wait for the EPDC to accept this update, right after it is sent.
    pub wait_submission_after: bool,
}

/// The FBInk waveform for a requested [`Waveform`].
///
/// `color` is true for `Rgb24` content. Two mappings change with it:
///
/// - `Gc16` (KOReader's `full`) becomes `GCC16`, as KOReader's `refresh_mtk` promotes it
///   when the dither flag is set on a color panel.
/// - `Glr16` (KOReader's `partial`, REAGL on MTK) becomes `GLRC16`, the color
///   counterpart that KOReader uses for the same case.
///
/// An explicit `Waveform::Gcc16` is always the color waveform, whatever the content.
pub fn waveform_for(waveform: Waveform, color: bool) -> u8 {
    match waveform {
        Waveform::Auto => fb::WFM_AUTO,
        Waveform::Du => fb::WFM_DU,
        Waveform::Gl16 => fb::WFM_GL16,
        Waveform::Gc16 if color => fb::WFM_GCC16,
        Waveform::Gc16 => fb::WFM_GC16,
        Waveform::A2 => fb::WFM_A2,
        Waveform::Glr16 if color => fb::WFM_GLRC16,
        Waveform::Glr16 => fb::WFM_REAGL,
        Waveform::Gcc16 => fb::WFM_GCC16,
    }
}

/// Widens a partial region outward to the [`REGION_ALIGN`] grid, clipped to the
/// screen. A region that covers the whole screen is returned unchanged.
///
/// KOReader does the same widening in `getBoundedRect` (`mxc_update`). The exact
/// rounding there is not verified; this function rounds outward on both axes.
pub fn align_region(region: Rect, screen: Size) -> Rect {
    if region.is_empty() || covers(region, screen) {
        return region;
    }
    let a = REGION_ALIGN;
    let x0 = (region.x.max(0) / a) * a;
    let y0 = (region.y.max(0) / a) * a;
    let x1 = (((region.right() + a - 1) / a) * a).min(screen.w as i32);
    let y1 = (((region.bottom() + a - 1) / a) * a).min(screen.h as i32);
    Rect::new(x0, y0, (x1 - x0).max(0) as u32, (y1 - y0).max(0) as u32)
}

/// Builds the plan for a refresh of `refresh.region` on a screen of `screen` logical
/// size, showing content in `format`.
///
/// The rules, each from `framebuffer_mxcfb.lua`:
///
/// - REAGL (`Glr16`) and Kaleido modes always run in FULL update mode. The flash flag
///   is forced on for them (`mxc_update`, "Handle promotion to FULL").
/// - Waits before sending: REAGL and GC16 updates, and a flashing full-screen AUTO
///   update (the UI flash). Those wait for the previous update first (`mxc_update`).
/// - After a FULL update, wait for it to complete (`mxc_update`, "fence off FULL
///   updates").
/// - After a PARTIAL update that is neither DU nor A2, wait for submission
///   (`wait_for_submission_after`, set for MTK).
/// - CFA post-processing is on for color content in a Kaleido mode (`CFA_PROCESSING_FLAG`,
///   set to `HWTCON_FLAG_CFA_EINK_G2` for color screens).
pub fn plan_refresh(format: PixelFormat, refresh: &Refresh, screen: Size) -> RefreshPlan {
    let color = format == PixelFormat::Rgb24;
    let waveform = waveform_for(refresh.waveform, color);
    let kaleido = waveform == fb::WFM_GCC16 || waveform == fb::WFM_GLRC16;
    let reagl = waveform == fb::WFM_REAGL;
    let flashing = refresh.flash || kaleido || reagl;
    let full = covers(refresh.region, screen);

    // The waits use the requested mode, before the color promotion, as KOReader does.
    let wait_before = matches!(refresh.waveform, Waveform::Gc16 | Waveform::Glr16)
        || (refresh.flash && refresh.waveform == Waveform::Auto && full);

    RefreshPlan {
        waveform,
        flashing,
        color_post: color && kaleido,
        wait_before,
        wait_after: flashing,
        wait_submission_after: !flashing && waveform != fb::WFM_DU && waveform != fb::WFM_A2,
    }
}

/// True when `region` is at least as large as `screen` in both directions.
fn covers(region: Rect, screen: Size) -> bool {
    region.w >= screen.w && region.h >= screen.h
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::waveform::Waveform;

    const SCREEN: Size = Size::new(1264, 1680);

    fn refresh(region: Rect, waveform: Waveform, flash: bool) -> Refresh {
        Refresh::new(region, waveform, flash)
    }

    #[test]
    fn gray_waveforms_map_to_fbink_modes() {
        assert_eq!(waveform_for(Waveform::Auto, false), fb::WFM_AUTO);
        assert_eq!(waveform_for(Waveform::Du, false), fb::WFM_DU);
        assert_eq!(waveform_for(Waveform::Gl16, false), fb::WFM_GL16);
        assert_eq!(waveform_for(Waveform::Gc16, false), fb::WFM_GC16);
        assert_eq!(waveform_for(Waveform::A2, false), fb::WFM_A2);
        assert_eq!(waveform_for(Waveform::Glr16, false), fb::WFM_REAGL);
        assert_eq!(waveform_for(Waveform::Gcc16, false), fb::WFM_GCC16);
    }

    #[test]
    fn color_content_promotes_full_and_partial_modes() {
        assert_eq!(waveform_for(Waveform::Gc16, true), fb::WFM_GCC16);
        assert_eq!(waveform_for(Waveform::Glr16, true), fb::WFM_GLRC16);
        // Fast modes stay black and white.
        assert_eq!(waveform_for(Waveform::Du, true), fb::WFM_DU);
        assert_eq!(waveform_for(Waveform::A2, true), fb::WFM_A2);
    }

    #[test]
    fn fast_gray_update_does_not_flash_or_wait() {
        let r = refresh(Rect::new(0, 0, 200, 40), Waveform::Du, false);
        let p = plan_refresh(PixelFormat::Gray8, &r, SCREEN);
        assert_eq!(p.waveform, fb::WFM_DU);
        assert!(!p.flashing);
        assert!(!p.wait_before && !p.wait_after && !p.wait_submission_after);
        assert!(!p.color_post);
    }

    #[test]
    fn a2_update_does_not_wait_for_submission() {
        let r = refresh(Rect::new(0, 0, 200, 40), Waveform::A2, false);
        let p = plan_refresh(PixelFormat::Gray8, &r, SCREEN);
        assert!(!p.wait_submission_after);
    }

    #[test]
    fn partial_gray_update_is_reagl_and_forced_full() {
        // KOReader's partial on MTK is REAGL, and REAGL always runs in FULL mode.
        let r = refresh(Rect::new(0, 0, 300, 300), Waveform::Glr16, false);
        let p = plan_refresh(PixelFormat::Gray8, &r, SCREEN);
        assert_eq!(p.waveform, fb::WFM_REAGL);
        assert!(p.flashing);
        assert!(p.wait_before);
        assert!(p.wait_after);
        assert!(!p.wait_submission_after);
    }

    #[test]
    fn auto_partial_waits_for_submission_only() {
        let r = refresh(Rect::new(10, 10, 300, 40), Waveform::Auto, false);
        let p = plan_refresh(PixelFormat::Gray8, &r, SCREEN);
        assert!(!p.flashing);
        assert!(!p.wait_before && !p.wait_after);
        assert!(p.wait_submission_after);
    }

    #[test]
    fn full_screen_flashing_ui_waits_before() {
        let r = refresh(Rect::new(0, 0, 1264, 1680), Waveform::Auto, true);
        let p = plan_refresh(PixelFormat::Gray8, &r, SCREEN);
        assert!(p.wait_before);
        assert!(p.flashing);
        assert!(p.wait_after);
    }

    #[test]
    fn partial_flashing_ui_does_not_wait_before() {
        let r = refresh(Rect::new(0, 0, 300, 300), Waveform::Auto, true);
        let p = plan_refresh(PixelFormat::Gray8, &r, SCREEN);
        assert!(!p.wait_before);
        assert!(p.flashing);
    }

    #[test]
    fn full_gray_refresh_is_gc16_and_waits_both_sides() {
        let r = refresh(Rect::new(0, 0, 1264, 1680), Waveform::Gc16, true);
        let p = plan_refresh(PixelFormat::Gray8, &r, SCREEN);
        assert_eq!(p.waveform, fb::WFM_GC16);
        assert!(p.wait_before && p.wait_after && p.flashing);
        assert!(!p.color_post);
    }

    #[test]
    fn color_full_refresh_uses_cfa_and_flashes() {
        let r = refresh(Rect::new(0, 0, 1264, 1680), Waveform::Gc16, true);
        let p = plan_refresh(PixelFormat::Rgb24, &r, SCREEN);
        assert_eq!(p.waveform, fb::WFM_GCC16);
        assert!(p.color_post);
        assert!(p.flashing);
        assert!(p.wait_before);
    }

    #[test]
    fn color_partial_uses_glrc16_with_cfa() {
        let r = refresh(Rect::new(0, 0, 400, 400), Waveform::Glr16, false);
        let p = plan_refresh(PixelFormat::Rgb24, &r, SCREEN);
        assert_eq!(p.waveform, fb::WFM_GLRC16);
        assert!(p.color_post);
        assert!(p.flashing, "Kaleido modes run in FULL update mode");
    }

    #[test]
    fn align_widens_to_the_grid_and_clips_at_the_edge() {
        assert_eq!(
            align_region(Rect::new(5, 5, 10, 10), SCREEN),
            Rect::new(0, 0, 16, 16)
        );
        assert_eq!(
            align_region(Rect::new(16, 16, 16, 16), SCREEN),
            Rect::new(16, 16, 16, 16)
        );
        // 1250 + 14 = 1264, which is already on the grid; 1670 + 10 = 1680 is too.
        assert_eq!(
            align_region(Rect::new(1250, 1670, 14, 10), SCREEN),
            Rect::new(1248, 1664, 16, 16)
        );
        // Right edge 1262 rounds to 1264, the screen width.
        assert_eq!(
            align_region(Rect::new(1200, 0, 62, 16), SCREEN),
            Rect::new(1200, 0, 64, 16)
        );
    }

    #[test]
    fn align_keeps_a_full_screen_region() {
        let full = Rect::new(0, 0, 1264, 1680);
        assert_eq!(align_region(full, SCREEN), full);
    }
}
