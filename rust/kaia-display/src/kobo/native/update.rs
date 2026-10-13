//! The update sequence of KOReader's `mxc_update` for the MediaTek panel, as pure code.
//!
//! [`Sequencer::plan_update`] takes a region and a request and returns the driver calls
//! in the order KOReader makes them, with the struct that goes to `HWTCON_SEND_UPDATE`.
//! The device code runs the calls. The sequencer keeps the state that KOReader keeps on
//! the framebuffer object between updates: the marker, the marker that was already
//! waited for, the `update_data.flags` word and the night mode state.
//!
//! Line numbers name `base/ffi/framebuffer_mxcfb.lua` unless a file is given.
//!
//! The dither flag is [`Refresh::dither`](crate::waveform::Refresh::dither), KOReader's
//! flag for image content. It turns on the Kaleido promotion and the hardware dithering
//! together (`mxc_update`, line 370, and `refresh_kobo_mtk`, line 707). The frame format
//! plays no part.

use kaia_gfx::Rect;

use super::abi::{
    HwtconRect, HwtconUpdateData, EPDC_FLAG_ENABLE_INVERSION, HWTCON_FLAG_CFA_EINK_G2,
    HWTCON_FLAG_USE_DITHERING, HWTCON_FLAG_USE_DITHERING_Y8_Y1_S, HWTCON_FLAG_USE_DITHERING_Y8_Y4_S,
    HWTCON_WAVEFORM_MODE_A2, HWTCON_WAVEFORM_MODE_AUTO, HWTCON_WAVEFORM_MODE_DU, HWTCON_WAVEFORM_MODE_GC16,
    HWTCON_WAVEFORM_MODE_GCC16, HWTCON_WAVEFORM_MODE_GCK16, HWTCON_WAVEFORM_MODE_GL16,
    HWTCON_WAVEFORM_MODE_GLKW16, HWTCON_WAVEFORM_MODE_GLR16, HWTCON_WAVEFORM_MODE_GLRC16, UPDATE_MODE_FULL,
    UPDATE_MODE_PARTIAL,
};
use super::geometry::{covers, Orientation};
use crate::waveform::Waveform;

/// One request as a `refresh*Imp` method hands it to `mxc_update`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Request {
    /// The waveform the caller asks for, before any promotion (`waveform_mode`).
    pub waveform: u32,
    /// `is_flashing`. FULL update mode and the flash waveforms depend on it.
    pub flashing: bool,
    /// `dither`. Turns on the Kaleido promotion and the hardware dithering.
    pub dither: bool,
}

/// One driver call, in the order it is made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Wake the EPDC by writing `fiti_power 1` to `/proc/hwtcon/cmd`
    /// (`kobo_mtk_wakeup_epdc`, line 139).
    Poweron,
    /// `HWTCON_WAIT_FOR_UPDATE_COMPLETE` for this marker (`kobo_mtk_wait_for_update_complete`,
    /// line 192). The result is ignored, as KOReader ignores it.
    WaitComplete(u32),
    /// `HWTCON_WAIT_FOR_UPDATE_SUBMISSION` for this marker (line 253). The result is ignored.
    WaitSubmission(u32),
    /// `HWTCON_SEND_UPDATE` with this struct.
    Send(HwtconUpdateData),
}

/// The `/proc/hwtcon/cmd` text that turns the hardware night mode on
/// (`device.lua`, `setHWNightmode`, line 720).
pub const NIGHT_MODE_ON_CMD: &str = "night_mode 4";
/// The `/proc/hwtcon/cmd` text that turns the hardware night mode off.
pub const NIGHT_MODE_OFF_CMD: &str = "night_mode 0";

/// The state that KOReader keeps on the framebuffer object between updates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sequencer {
    /// `fb.marker`: the last marker handed out. 0 before the first update (line 57).
    marker: u32,
    /// `fb.dont_wait_for_marker`: the marker that was already waited for.
    dont_wait_for_marker: Option<u32>,
    /// `fb.update_data.flags`. KOReader reuses the struct, so this value survives
    /// from one update to the next.
    flags: u32,
    /// `fb.night_mode`.
    night_mode: bool,
    /// `fb:getHWNightmode()`: the state last written to the night mode knob on MTK
    /// (`device.lua`, `setHWNightmode` for MTK, line 713 onward).
    hw_night_mode: bool,
    /// `fb:isColorEnabled()`: the color rendering setting (line 370). The Kaleido
    /// promotion needs it on. The app sets it from its settings.
    color_enabled: bool,
}

impl Default for Sequencer {
    /// Color rendering on, which is KOReader's default on a color panel.
    fn default() -> Self {
        Sequencer {
            marker: 0,
            dont_wait_for_marker: None,
            flags: 0,
            night_mode: false,
            hw_night_mode: false,
            color_enabled: true,
        }
    }
}

impl Sequencer {
    /// The last marker handed out. 0 before the first update.
    pub fn marker(&self) -> u32 {
        self.marker
    }

    /// The night mode that the sequencer applies to updates.
    pub fn night_mode(&self) -> bool {
        self.night_mode
    }

    /// Whether the color rendering setting is on.
    pub fn color_enabled(&self) -> bool {
        self.color_enabled
    }

    /// Sets the color rendering setting (`isColorEnabled`, line 370). With it off, the
    /// dither flag still turns on hardware dithering, but REAGL and GC16 are not
    /// promoted to the Kaleido modes and the CFA flag stays clear.
    pub fn set_color_enabled(&mut self, on: bool) {
        self.color_enabled = on;
    }

    /// Resets the night mode for shutdown, as `Device:exit` does with
    /// `setHWNightmode(orig_hw_nightmode)` (`generic/device.lua`, line 667). The
    /// original state is always off on MTK, because Nickel inverts in software. Returns
    /// the text to write to `/proc/hwtcon/cmd`. KOReader writes it whatever the state.
    pub fn reset_night_mode_on_exit(&mut self) -> &'static str {
        self.night_mode = false;
        self.hw_night_mode = false;
        NIGHT_MODE_OFF_CMD
    }

    /// Sets the night mode, as `toggleNightMode` does on MTK: the frame state and the
    /// HW state change together. The device writes `night_mode 4` or `night_mode 0`
    /// to `/proc/hwtcon/cmd`.
    pub fn set_night_mode(&mut self, on: bool) {
        self.night_mode = on;
        self.hw_night_mode = on;
    }

    /// Plans one `mxc_update` call for the bounded logical region `region`.
    ///
    /// `region` must already be bounded and aligned (`getBoundedRect`, line 283). The
    /// caller has written those pixels to the framebuffer already, as KOReader paints
    /// before it refreshes.
    ///
    /// Returns `None` when KOReader would discard the update: its native width or
    /// height is 1 or less (line 291). Nothing else changes in that case except the
    /// flags word, which `refresh_kobo_mtk` sets before `mxc_update` runs.
    pub fn plan_update(
        &mut self,
        orientation: &Orientation,
        region: Rect,
        request: Request,
    ) -> Option<Vec<Step>> {
        // refresh_kobo_mtk (line 707) runs before mxc_update. It sets the dither bit
        // with OR, so the bits of the previous update stay. Without dither it clears
        // the word. This happens before the discard check, so it persists either way.
        let dither_mode = if !request.dither {
            self.flags = 0;
            0
        } else {
            self.flags |= HWTCON_FLAG_USE_DITHERING;
            if request.waveform == HWTCON_WAVEFORM_MODE_DU {
                HWTCON_FLAG_USE_DITHERING_Y8_Y1_S
            } else {
                HWTCON_FLAG_USE_DITHERING_Y8_Y4_S
            }
        };

        let physical = orientation.physical_rect(region);
        if physical.w <= 1 || physical.h <= 1 {
            return None;
        }

        let mut steps = vec![Step::Poweron];
        let mut flags = self.flags;
        let requested = request.waveform;

        // Wait for the previous update before REAGL, GC16 and a flashing full-screen
        // UI update (line 341). The test uses the requested waveform, before promotion.
        // The full-screen test runs after the alignment, as line 341 does.
        let full_screen = covers(region, orientation.logical_size());
        let wait_before = requested == HWTCON_WAVEFORM_MODE_GLR16
            || requested == HWTCON_WAVEFORM_MODE_GC16
            || (request.flashing && requested == HWTCON_WAVEFORM_MODE_AUTO && full_screen);
        if wait_before && self.marker != 0 && Some(self.marker) != self.dont_wait_for_marker {
            steps.push(Step::WaitComplete(self.marker));
        }

        // Each update takes the next marker (line 359).
        self.marker = next_marker(self.marker);
        let marker = self.marker;

        let mut waveform = requested;
        let mut update_mode = if request.flashing {
            UPDATE_MODE_FULL
        } else {
            UPDATE_MODE_PARTIAL
        };

        // Kaleido promotion (line 370): REAGL becomes GLRC16, GC16 becomes GCC16. It needs
        // the dither flag, a Kaleido-capable panel (the Libra Colour is) and the color
        // rendering setting (`isColorEnabled`, `set_color_enabled`). The CFA bit is set
        // for either Kaleido mode.
        if request.dither && self.color_enabled {
            if waveform == HWTCON_WAVEFORM_MODE_GLR16 {
                waveform = HWTCON_WAVEFORM_MODE_GLRC16;
            } else if waveform == HWTCON_WAVEFORM_MODE_GC16 {
                waveform = HWTCON_WAVEFORM_MODE_GCC16;
            }
            if is_kaleido(waveform) {
                flags |= HWTCON_FLAG_CFA_EINK_G2;
            }
        }

        // Night mode (line 389). Inversion is done by the driver through the flag.
        // When the HW state is already set, the per-update flag is not needed.
        if self.night_mode {
            if !self.hw_night_mode {
                flags |= EPDC_FLAG_ENABLE_INVERSION;
            }
            // Kaleido modes stay as they are.
            if !is_kaleido(waveform) {
                if waveform == HWTCON_WAVEFORM_MODE_GLR16 {
                    // The partial waveform, with the night waveform (line 408 onward).
                    // On MTK the night REAGL forces FULL update mode.
                    waveform = HWTCON_WAVEFORM_MODE_GLKW16;
                    update_mode = UPDATE_MODE_FULL;
                } else if waveform == HWTCON_WAVEFORM_MODE_GC16 || request.flashing {
                    waveform = HWTCON_WAVEFORM_MODE_GCK16;
                }
            }
        }

        // REAGL and Kaleido updates are always FULL (line 417).
        if waveform == HWTCON_WAVEFORM_MODE_GLR16 || is_kaleido(waveform) {
            update_mode = UPDATE_MODE_FULL;
        }

        self.flags = flags;
        let data = HwtconUpdateData {
            update_region: HwtconRect {
                top: physical.y as u32,
                left: physical.x as u32,
                width: physical.w,
                height: physical.h,
            },
            waveform_mode: waveform,
            update_mode,
            update_marker: marker,
            flags,
            dither_mode,
        };
        steps.push(Step::Send(data));

        // Fence off FULL updates (line 439). The wait also marks the marker as waited.
        if update_mode == UPDATE_MODE_FULL {
            steps.push(Step::WaitComplete(marker));
            self.dont_wait_for_marker = Some(marker);
        }

        // After a PARTIAL update that is neither DU nor A2, wait for its submission
        // (line 452). KOReader also tests `marker ~= 0`. The new marker is never 0, so
        // that test is dropped.
        if update_mode == UPDATE_MODE_PARTIAL
            && waveform != HWTCON_WAVEFORM_MODE_DU
            && waveform != HWTCON_WAVEFORM_MODE_A2
            && Some(marker) != self.dont_wait_for_marker
        {
            steps.push(Step::WaitSubmission(marker));
        }

        Some(steps)
    }

    /// `refreshWaitForLastImp` (`framebuffer_mxcfb.lua`, line 814): waits for the last
    /// marker, unless it was waited for already.
    pub fn plan_wait_for_last(&mut self) -> Vec<Step> {
        if self.dont_wait_for_marker == Some(self.marker) {
            return Vec::new();
        }
        self.dont_wait_for_marker = Some(self.marker);
        vec![Step::WaitComplete(self.marker)]
    }
}

/// True for the Kaleido color modes (`_isKaleidoWaveFormMode`, line 85 onward).
fn is_kaleido(waveform: u32) -> bool {
    waveform == HWTCON_WAVEFORM_MODE_GCC16 || waveform == HWTCON_WAVEFORM_MODE_GLRC16
}

/// The next marker. The counter wraps to 1 after `0xFFFFFFFF` (`_get_next_marker`,
/// line 57).
fn next_marker(marker: u32) -> u32 {
    if marker == u32::MAX {
        1
    } else {
        marker + 1
    }
}

/// The KOReader request for a Kaia refresh.
///
/// - `Auto` is the `ui` waveform (`AUTO`). Its flashing form is the `flashui` waveform,
///   which is also `AUTO`.
/// - `Du` is `fast`, `A2` is `a2`.
/// - `Gc16` is `full`, and `Glr16` is `partial`, which is REAGL on MTK.
/// - `Gcc16` asks for GC16 with the dither flag. The promotion then gives GCC16, as
///   KOReader does for dithered content.
///
/// `dither` is [`Refresh::dither`](crate::waveform::Refresh::dither). `Gcc16` names the
/// color waveform, so it always carries the dither flag.
pub fn request_for(waveform: Waveform, flash: bool, dither: bool) -> Request {
    let (waveform, dither) = match waveform {
        Waveform::Auto => (HWTCON_WAVEFORM_MODE_AUTO, dither),
        Waveform::Du => (HWTCON_WAVEFORM_MODE_DU, dither),
        Waveform::Gl16 => (HWTCON_WAVEFORM_MODE_GL16, dither),
        Waveform::Gc16 => (HWTCON_WAVEFORM_MODE_GC16, dither),
        Waveform::A2 => (HWTCON_WAVEFORM_MODE_A2, dither),
        Waveform::Glr16 => (HWTCON_WAVEFORM_MODE_GLR16, dither),
        Waveform::Gcc16 => (HWTCON_WAVEFORM_MODE_GC16, true),
    };
    Request {
        waveform,
        flashing: flash,
        dither,
    }
}

#[cfg(test)]
mod tests {
    use kaia_gfx::Size;

    use super::super::geometry::bounded_rect;
    use super::*;
    use crate::rotation::Rotation;

    const PANEL: Size = Size::new(1264, 1680);
    const FULL: Rect = Rect::new(0, 0, 1264, 1680);

    fn portrait() -> Orientation {
        Orientation::new(PANEL, Rotation::Deg0)
    }

    /// The device side: bound the region, then plan.
    fn run(s: &mut Sequencer, region: Rect, request: Request) -> Option<Vec<Step>> {
        let bounded = bounded_rect(region, portrait().logical_size(), Some(16));
        s.plan_update(&portrait(), bounded, request)
    }

    fn req(waveform: u32, flashing: bool, dither: bool) -> Request {
        Request {
            waveform,
            flashing,
            dither,
        }
    }

    /// The expected `HWTCON_SEND_UPDATE` struct, written field by field.
    #[allow(clippy::too_many_arguments)]
    fn send(
        top: u32,
        left: u32,
        width: u32,
        height: u32,
        waveform: u32,
        update_mode: u32,
        marker: u32,
        flags: u32,
        dither_mode: i32,
    ) -> Step {
        Step::Send(HwtconUpdateData {
            update_region: HwtconRect {
                top,
                left,
                width,
                height,
            },
            waveform_mode: waveform,
            update_mode,
            update_marker: marker,
            flags,
            dither_mode,
        })
    }

    #[test]
    fn a_session_of_gray_updates_matches_koreader() {
        let mut s = Sequencer::default();

        // 1. Partial gray update, requested as partial (GLR16 = REAGL). Bounded to
        //    (96, 96, 304, 304). The first update waits for nothing: the marker is 0
        //    (line 341). REAGL forces FULL (line 417), so the update waits for itself.
        let steps = run(
            &mut s,
            Rect::new(100, 100, 300, 300),
            req(HWTCON_WAVEFORM_MODE_GLR16, false, false),
        )
        .unwrap();
        assert_eq!(
            steps,
            vec![
                Step::Poweron,
                send(
                    96,
                    96,
                    304,
                    304,
                    HWTCON_WAVEFORM_MODE_GLR16,
                    UPDATE_MODE_FULL,
                    1,
                    0,
                    0
                ),
                Step::WaitComplete(1),
            ]
        );
        assert_eq!(s.marker(), 1);

        // 2. UI update (AUTO), partial, bounded to (192, 192, 80, 48). Not DU or A2, so
        //    it waits for its submission (line 452). The marker of update 1 was waited
        //    for, so no wait before (line 341 does not apply to AUTO).
        let steps = run(
            &mut s,
            Rect::new(200, 200, 64, 32),
            req(HWTCON_WAVEFORM_MODE_AUTO, false, false),
        )
        .unwrap();
        assert_eq!(
            steps,
            vec![
                Step::Poweron,
                send(
                    192,
                    192,
                    80,
                    48,
                    HWTCON_WAVEFORM_MODE_AUTO,
                    UPDATE_MODE_PARTIAL,
                    2,
                    0,
                    0
                ),
                Step::WaitSubmission(2),
            ]
        );

        // 3. Fast DU update, bounded to (0, 0, 112, 112). DU never waits for submission.
        let steps = run(
            &mut s,
            Rect::new(0, 0, 100, 100),
            req(HWTCON_WAVEFORM_MODE_DU, false, false),
        )
        .unwrap();
        assert_eq!(
            steps,
            vec![
                Step::Poweron,
                send(
                    0,
                    0,
                    112,
                    112,
                    HWTCON_WAVEFORM_MODE_DU,
                    UPDATE_MODE_PARTIAL,
                    3,
                    0,
                    0
                ),
            ]
        );

        // 4. Full-screen GC16 flash. Waits for marker 3 before it starts (line 341),
        //    sends marker 4 as FULL, and waits for marker 4 (line 439).
        let steps = run(&mut s, FULL, req(HWTCON_WAVEFORM_MODE_GC16, true, false)).unwrap();
        assert_eq!(
            steps,
            vec![
                Step::Poweron,
                Step::WaitComplete(3),
                send(
                    0,
                    0,
                    1264,
                    1680,
                    HWTCON_WAVEFORM_MODE_GC16,
                    UPDATE_MODE_FULL,
                    4,
                    0,
                    0
                ),
                Step::WaitComplete(4),
            ]
        );
    }

    #[test]
    fn color_full_flash_promotes_to_gcc16_with_the_cfa_flag() {
        let mut s = Sequencer::default();
        run(
            &mut s,
            Rect::new(0, 0, 64, 64),
            req(HWTCON_WAVEFORM_MODE_DU, false, false),
        );
        run(&mut s, FULL, req(HWTCON_WAVEFORM_MODE_GC16, true, false));

        // Dithered GC16 (color). The dither word is set (line 710): flags = 0 | 1, and
        // dither mode Y8_Y4_S because the waveform is not DU (line 714). The promotion
        // makes GCC16 (line 370). The CFA bit makes flags 1 | 1536 = 1537. Marker 2 was
        // the FULL update before it and was waited for, so there is no wait before.
        let steps = run(&mut s, FULL, req(HWTCON_WAVEFORM_MODE_GC16, true, true)).unwrap();
        assert_eq!(
            steps,
            vec![
                Step::Poweron,
                send(
                    0,
                    0,
                    1264,
                    1680,
                    HWTCON_WAVEFORM_MODE_GCC16,
                    UPDATE_MODE_FULL,
                    3,
                    1537,
                    0x102
                ),
                Step::WaitComplete(3),
            ]
        );
    }

    /// The `HWTCON_SEND_UPDATE` struct of the single send in `steps`.
    fn sent(steps: &[Step]) -> HwtconUpdateData {
        steps
            .iter()
            .find_map(|s| match s {
                Step::Send(d) => Some(*d),
                _ => None,
            })
            .expect("one send step")
    }

    #[test]
    fn color_rendering_off_blocks_the_kaleido_promotion() {
        // Line 370 needs isColorEnabled(). With it off, dithered GC16 stays GC16 and the
        // CFA bit stays clear. The hardware dithering flag still applies (line 710).
        let mut s = Sequencer::default();
        assert!(s.color_enabled(), "color rendering starts on");
        s.set_color_enabled(false);
        let d = sent(&run(&mut s, FULL, req(HWTCON_WAVEFORM_MODE_GC16, true, true)).unwrap());
        assert_eq!(d.waveform_mode, HWTCON_WAVEFORM_MODE_GC16);
        assert_eq!(d.flags, HWTCON_FLAG_USE_DITHERING);
        assert_eq!(d.flags & HWTCON_FLAG_CFA_EINK_G2, 0);

        // Dithered REAGL stays REAGL, and REAGL is FULL either way (line 417).
        let d = sent(
            &run(
                &mut s,
                Rect::new(0, 0, 400, 400),
                req(HWTCON_WAVEFORM_MODE_GLR16, false, true),
            )
            .unwrap(),
        );
        assert_eq!(d.waveform_mode, HWTCON_WAVEFORM_MODE_GLR16);
        assert_eq!(d.update_mode, UPDATE_MODE_FULL);
        assert_eq!(d.flags & HWTCON_FLAG_CFA_EINK_G2, 0);
    }

    #[test]
    fn color_rendering_back_on_restores_the_promotion() {
        let mut s = Sequencer::default();
        s.set_color_enabled(false);
        s.set_color_enabled(true);
        let d = sent(&run(&mut s, FULL, req(HWTCON_WAVEFORM_MODE_GC16, true, true)).unwrap());
        assert_eq!(d.waveform_mode, HWTCON_WAVEFORM_MODE_GCC16);
        assert_ne!(d.flags & HWTCON_FLAG_CFA_EINK_G2, 0);
    }

    #[test]
    fn exit_resets_the_night_mode() {
        let mut s = Sequencer::default();
        s.set_night_mode(true);
        assert!(s.night_mode());
        assert_eq!(s.reset_night_mode_on_exit(), NIGHT_MODE_OFF_CMD);
        assert!(!s.night_mode());
        // The next update carries no inversion flag and no night waveform.
        let d = sent(&run(&mut s, FULL, req(HWTCON_WAVEFORM_MODE_GC16, true, false)).unwrap());
        assert_eq!(d.waveform_mode, HWTCON_WAVEFORM_MODE_GC16);
        assert_eq!(d.flags & EPDC_FLAG_ENABLE_INVERSION, 0);
        // KOReader writes the knob even when the mode was already off.
        assert_eq!(s.reset_night_mode_on_exit(), "night_mode 0");
    }

    #[test]
    fn night_mode_knob_texts_match_device_lua() {
        assert_eq!(NIGHT_MODE_ON_CMD, "night_mode 4");
        assert_eq!(NIGHT_MODE_OFF_CMD, "night_mode 0");
    }

    #[test]
    fn dithered_updates_keep_the_flag_bits_of_the_previous_update() {
        // refresh_kobo_mtk ORs into update_data.flags (line 710). The CFA bit from the
        // GCC16 update stays, so a later dithered DU update carries it too.
        let mut s = Sequencer::default();
        run(&mut s, FULL, req(HWTCON_WAVEFORM_MODE_GC16, true, true));
        let steps = run(
            &mut s,
            Rect::new(0, 0, 64, 64),
            req(HWTCON_WAVEFORM_MODE_DU, false, true),
        )
        .unwrap();
        assert_eq!(
            steps,
            vec![
                Step::Poweron,
                send(
                    0,
                    0,
                    64,
                    64,
                    HWTCON_WAVEFORM_MODE_DU,
                    UPDATE_MODE_PARTIAL,
                    2,
                    1537,
                    0x302
                ),
            ]
        );
        // A gray update clears the word (line 719).
        let steps = run(
            &mut s,
            Rect::new(0, 0, 64, 64),
            req(HWTCON_WAVEFORM_MODE_DU, false, false),
        )
        .unwrap();
        assert_eq!(
            steps,
            vec![
                Step::Poweron,
                send(
                    0,
                    0,
                    64,
                    64,
                    HWTCON_WAVEFORM_MODE_DU,
                    UPDATE_MODE_PARTIAL,
                    3,
                    0,
                    0
                ),
            ]
        );
    }

    #[test]
    fn a_dithered_partial_ui_update_uses_the_dither_mode_and_no_promotion() {
        let mut s = Sequencer::default();
        let steps = run(
            &mut s,
            Rect::new(0, 0, 64, 64),
            req(HWTCON_WAVEFORM_MODE_AUTO, false, true),
        )
        .unwrap();
        assert_eq!(
            steps,
            vec![
                Step::Poweron,
                send(
                    0,
                    0,
                    64,
                    64,
                    HWTCON_WAVEFORM_MODE_AUTO,
                    UPDATE_MODE_PARTIAL,
                    1,
                    1,
                    0x102
                ),
                Step::WaitSubmission(1),
            ]
        );
    }

    #[test]
    fn partial_color_refresh_is_glrc16_in_full_mode() {
        let mut s = Sequencer::default();
        let steps = run(
            &mut s,
            Rect::new(0, 0, 400, 400),
            request_for(Waveform::Glr16, false, true),
        )
        .unwrap();
        assert_eq!(
            steps,
            vec![
                Step::Poweron,
                send(
                    0,
                    0,
                    400,
                    400,
                    HWTCON_WAVEFORM_MODE_GLRC16,
                    UPDATE_MODE_FULL,
                    1,
                    1537,
                    0x102
                ),
                Step::WaitComplete(1),
            ]
        );
    }

    #[test]
    fn a_flashing_full_screen_ui_update_waits_before_but_a_partial_one_does_not() {
        let mut s = Sequencer::default();
        run(
            &mut s,
            Rect::new(0, 0, 64, 64),
            req(HWTCON_WAVEFORM_MODE_AUTO, false, false),
        );
        // Full screen flash AUTO: waits for marker 1 (line 341, flashui with full screen).
        let steps = run(&mut s, FULL, req(HWTCON_WAVEFORM_MODE_AUTO, true, false)).unwrap();
        assert_eq!(steps[1], Step::WaitComplete(1));

        // A flash AUTO on a partial region does not wait before.
        let steps = run(
            &mut s,
            Rect::new(0, 0, 64, 64),
            req(HWTCON_WAVEFORM_MODE_AUTO, true, false),
        )
        .unwrap();
        assert_eq!(
            steps[1],
            send(0, 0, 64, 64, HWTCON_WAVEFORM_MODE_AUTO, UPDATE_MODE_FULL, 3, 0, 0)
        );
    }

    #[test]
    fn a_region_that_aligns_to_the_screen_counts_as_full_for_the_wait() {
        // 1260 x 1676 bounds to the full screen, so a flashing AUTO update waits before.
        let mut s = Sequencer::default();
        run(
            &mut s,
            Rect::new(0, 0, 64, 64),
            req(HWTCON_WAVEFORM_MODE_AUTO, false, false),
        );
        let steps = run(
            &mut s,
            Rect::new(0, 0, 1260, 1676),
            req(HWTCON_WAVEFORM_MODE_AUTO, true, false),
        )
        .unwrap();
        assert_eq!(steps[1], Step::WaitComplete(1));
    }

    #[test]
    fn night_mode_swaps_partial_to_glkw16_and_forces_full() {
        let mut s = Sequencer::default();
        s.set_night_mode(true);
        // The HW state is set too, so the inversion flag is not sent (line 392).
        let steps = run(
            &mut s,
            Rect::new(0, 0, 300, 300),
            req(HWTCON_WAVEFORM_MODE_GLR16, false, false),
        )
        .unwrap();
        assert_eq!(
            steps,
            vec![
                Step::Poweron,
                send(
                    0,
                    0,
                    304,
                    304,
                    HWTCON_WAVEFORM_MODE_GLKW16,
                    UPDATE_MODE_FULL,
                    1,
                    0,
                    0
                ),
                Step::WaitComplete(1),
            ]
        );
    }

    #[test]
    fn night_mode_turns_flashes_into_gck16() {
        let mut s = Sequencer::default();
        s.set_night_mode(true);
        let steps = run(&mut s, FULL, req(HWTCON_WAVEFORM_MODE_GC16, true, false)).unwrap();
        assert_eq!(
            steps[1],
            send(
                0,
                0,
                1264,
                1680,
                HWTCON_WAVEFORM_MODE_GCK16,
                UPDATE_MODE_FULL,
                1,
                0,
                0
            )
        );
    }

    #[test]
    fn night_mode_without_the_hw_state_sets_the_inversion_flag() {
        // Not reachable through set_night_mode, which keeps both states together. This is
        // the branch of line 392 when getHWNightmode() is false.
        let mut s = Sequencer {
            night_mode: true,
            ..Default::default()
        };
        let steps = run(
            &mut s,
            Rect::new(0, 0, 64, 64),
            req(HWTCON_WAVEFORM_MODE_DU, false, false),
        )
        .unwrap();
        assert_eq!(
            steps[1],
            send(
                0,
                0,
                64,
                64,
                HWTCON_WAVEFORM_MODE_DU,
                UPDATE_MODE_PARTIAL,
                1,
                EPDC_FLAG_ENABLE_INVERSION,
                0
            )
        );
    }

    #[test]
    fn a_discarded_update_consumes_no_marker_but_keeps_the_flag_change() {
        // A region fully off screen bounds to an empty rect (line 291 discards it).
        let mut s = Sequencer::default();
        run(&mut s, FULL, req(HWTCON_WAVEFORM_MODE_GC16, true, true));
        let marker_before = s.marker();
        let out = run(
            &mut s,
            Rect::new(2000, 10, 5, 5),
            req(HWTCON_WAVEFORM_MODE_DU, false, true),
        );
        assert!(out.is_none());
        assert_eq!(s.marker(), marker_before);
        // refresh_kobo_mtk ran before the discard, so the dither bit is set and the
        // CFA bit from before is still there.
        assert_eq!(s.flags, 1537);
    }

    #[test]
    fn wait_for_last_waits_once_per_marker() {
        let mut s = Sequencer::default();
        // Before any update the marker is 0, and nothing has been waited for. KOReader
        // waits on marker 0 here, since dont_wait_for_marker is nil (line 814).
        assert_eq!(s.plan_wait_for_last(), vec![Step::WaitComplete(0)]);
        assert!(s.plan_wait_for_last().is_empty());

        run(
            &mut s,
            Rect::new(0, 0, 64, 64),
            req(HWTCON_WAVEFORM_MODE_DU, false, false),
        );
        assert_eq!(s.plan_wait_for_last(), vec![Step::WaitComplete(1)]);
        assert!(s.plan_wait_for_last().is_empty());

        // A FULL update already waited for its marker, so nothing is left to wait for.
        run(&mut s, FULL, req(HWTCON_WAVEFORM_MODE_GC16, true, false));
        assert!(s.plan_wait_for_last().is_empty());
    }

    #[test]
    fn the_marker_wraps_to_one_after_the_maximum() {
        assert_eq!(next_marker(0), 1);
        assert_eq!(next_marker(41), 42);
        assert_eq!(next_marker(u32::MAX), 1);
        let mut s = Sequencer {
            marker: u32::MAX,
            ..Default::default()
        };
        let steps = run(
            &mut s,
            Rect::new(0, 0, 64, 64),
            req(HWTCON_WAVEFORM_MODE_DU, false, false),
        )
        .unwrap();
        assert_eq!(
            steps[1],
            send(
                0,
                0,
                64,
                64,
                HWTCON_WAVEFORM_MODE_DU,
                UPDATE_MODE_PARTIAL,
                1,
                0,
                0
            )
        );
    }

    #[test]
    fn requests_follow_the_koreader_refresh_methods() {
        assert_eq!(request_for(Waveform::Auto, false, false), req(257, false, false));
        assert_eq!(request_for(Waveform::Du, false, false), req(1, false, false));
        assert_eq!(request_for(Waveform::A2, false, false), req(6, false, false));
        assert_eq!(request_for(Waveform::Gc16, true, false), req(2, true, false));
        assert_eq!(request_for(Waveform::Glr16, false, false), req(4, false, false));
        // The dither flag passes through.
        assert_eq!(request_for(Waveform::Du, false, true), req(1, false, true));
        // Gcc16 is GC16 with the dither flag, so it is promoted.
        assert_eq!(request_for(Waveform::Gcc16, true, false), req(2, true, true));
    }

    #[test]
    fn a_gcc16_request_from_a_gray_frame_is_promoted() {
        let mut s = Sequencer::default();
        let steps = run(&mut s, FULL, request_for(Waveform::Gcc16, true, false)).unwrap();
        assert_eq!(
            steps[1],
            send(
                0,
                0,
                1264,
                1680,
                HWTCON_WAVEFORM_MODE_GCC16,
                UPDATE_MODE_FULL,
                1,
                1537,
                0x102
            )
        );
    }
}
