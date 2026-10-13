//! The Kobo E Ink panel, driven through FBInk. Linux only.
//!
//! Frames arrive in logical orientation. The backend turns the changed region into
//! panel order in software, so the framebuffer keeps its native rotation. It draws the
//! region with `fbink_print_raw_data` (no refresh), waits as KOReader does, and then
//! refreshes the region with `fbink_refresh`. The waveform and flash come from
//! [`plan_refresh`](crate::kobo::plan::plan_refresh).
//!
//! Nothing here has run on a device. See the list in [`crate::kobo`].

use std::io;
use std::os::raw::c_int;

use kaia_fbink_sys as fb;
use kaia_gfx::{PixelFormat, Pixmap, Rect, Size};

use crate::display::{check_present, Display, DisplayError, PanelSpec};
use crate::kobo::plan::{align_region, plan_refresh, RefreshPlan};
use crate::rotation::{logical_rect_to_panel, rotate_to_panel, Rotation};
use crate::waveform::Refresh;

/// A Kobo panel behind FBInk.
///
/// Opening it takes the framebuffer and holds it until the value is dropped. The
/// framebuffer must be in its native portrait orientation, with the panel size in
/// [`PanelSpec`]. [`KoboDisplay::open`] checks the size and refuses other values.
pub struct KoboDisplay {
    fbfd: c_int,
    panel: PanelSpec,
    rotation: Rotation,
    /// The config that `fbink_init` saw. Drawing copies it and changes only the fields
    /// that a call needs.
    cfg: fb::FBInkConfig,
    state: fb::FBInkState,
}

impl KoboDisplay {
    /// Opens the framebuffer and initializes FBInk for the given panel.
    ///
    /// Fails when the FBInk build lacks image support, when the framebuffer cannot be
    /// opened, or when its size is not the panel's size at native rotation.
    pub fn open(panel: PanelSpec) -> Result<Self, DisplayError> {
        // SAFETY: `fbink_features` takes no arguments and reads only FBInk's build-time
        // constants.
        let features = unsafe { fb::fbink_features() };
        if features & fb::FBINK_FEATURE_IMAGE == 0 {
            return Err(backend(
                "this FBInk build has no image support, and fbink_print_raw_data needs it (build with IMAGE=1)",
            ));
        }

        // SAFETY: `fbink_open` takes no arguments. It returns an fd or a negative value.
        // On success, the fd belongs to the returned struct, and Drop closes it once.
        let fbfd = unsafe { fb::fbink_open() };
        if fbfd < 0 {
            return Err(backend(format!(
                "fbink_open failed: {}",
                io::Error::last_os_error()
            )));
        }

        let cfg = fb::FBInkConfig {
            is_quiet: true,
            ..Default::default()
        };
        let mut dev = Self {
            fbfd,
            panel,
            rotation: Rotation::Deg0,
            cfg,
            state: fb::FBInkState::zeroed(),
        };

        // SAFETY: `fbfd` is open and owned by `dev`. `cfg` lives for the call, and FBInk
        // reads it only during the call.
        let rc = unsafe { fb::fbink_init(dev.fbfd, &dev.cfg) };
        if rc < 0 {
            return Err(backend(format!("fbink_init failed ({rc})")));
        }

        // SAFETY: `cfg` and `state` are valid for the call. `FBInkState` in kaia-fbink-sys
        // matches the header's size on each pointer width, and FBInk writes only that
        // struct.
        unsafe { fb::fbink_get_state(&dev.cfg, &mut dev.state) };

        let (w, h) = (dev.state.screen_width, dev.state.screen_height);
        if (w, h) != (panel.size.w, panel.size.h) {
            return Err(backend(format!(
                "framebuffer is {w}x{h}, expected {}x{} at native rotation",
                panel.size.w, panel.size.h
            )));
        }
        Ok(dev)
    }

    /// Opens the Libra Colour panel (1264 by 1680, 300 dpi).
    pub fn libra_colour() -> Result<Self, DisplayError> {
        Self::open(PanelSpec::LIBRA_COLOUR)
    }

    /// The current rotation. It is set by [`Display::set_rotation`].
    pub fn rotation(&self) -> Rotation {
        self.rotation
    }

    /// Bits per pixel of the framebuffer, as FBInk reports it.
    pub fn framebuffer_bpp(&self) -> u32 {
        self.state.bpp
    }

    /// True when FBInk identifies the SoC as MediaTek.
    pub fn is_mtk(&self) -> bool {
        self.state.is_mtk
    }

    /// True when FBInk reports a color panel.
    pub fn has_color_panel(&self) -> bool {
        self.state.has_color_panel
    }

    /// Draws `pixels` at `at` without refreshing the panel.
    fn draw(&self, pixels: &Pixmap, at: Rect) -> Result<(), DisplayError> {
        let mut cfg = self.cfg;
        cfg.no_refresh = true;
        cfg.ignore_alpha = true;
        let len = pixels.data().len();
        // SAFETY: `pixels.data()` holds exactly `w * h * bpp` bytes, and `len` is that
        // length, which FBInk needs to pick the input format. `at` lies on the panel, so
        // its coordinates fit in `c_short`. `cfg` lives for the call, and `fbfd` is open.
        let rc = unsafe {
            fb::fbink_print_raw_data(
                self.fbfd,
                pixels.data().as_ptr(),
                pixels.width() as c_int,
                pixels.height() as c_int,
                len,
                at.x as _,
                at.y as _,
                &cfg,
            )
        };
        if rc < 0 {
            return Err(backend(format!("fbink_print_raw_data failed ({rc})")));
        }
        Ok(())
    }

    fn refresh(&self, at: Rect, plan: &RefreshPlan) -> Result<(), DisplayError> {
        let mut cfg = self.cfg;
        cfg.no_refresh = false;
        cfg.wfm_mode = plan.waveform;
        cfg.is_flashing = plan.flashing;
        cfg.dithering_mode = plan.dithering_mode;
        cfg.cfa_mode = if plan.color_post {
            fb::CFA_G2
        } else {
            fb::CFA_DEFAULT
        };
        // SAFETY: `fbfd` is open. `at` is a panel rectangle inside the panel, because
        // `present` aligns and clips the region first. `cfg` lives for the call.
        let rc = unsafe { fb::fbink_refresh(self.fbfd, at.y as u32, at.x as u32, at.w, at.h, &cfg) };
        if rc < 0 {
            return Err(backend(format!("fbink_refresh failed ({rc})")));
        }
        Ok(())
    }

    fn wait_complete(&self, marker: u32) {
        // SAFETY: `fbfd` is open. FBInk takes the marker by value and reads no pointers.
        // Its result is ignored on purpose, as KOReader ignores the matching ioctl.
        let _ = unsafe { fb::fbink_wait_for_complete(self.fbfd, marker) };
    }

    fn wait_submission(&self, marker: u32) {
        // SAFETY: as in `wait_complete`.
        let _ = unsafe { fb::fbink_wait_for_submission(self.fbfd, marker) };
    }

    fn last_marker(&self) -> u32 {
        // SAFETY: `fbink_get_last_marker` takes no arguments and reads FBInk's own state.
        unsafe { fb::fbink_get_last_marker() }
    }
}

impl Drop for KoboDisplay {
    fn drop(&mut self) {
        // SAFETY: `fbfd` came from `fbink_open` and is closed only here. After this,
        // the struct is gone, so nothing uses it again.
        unsafe {
            fb::fbink_close(self.fbfd);
        }
    }
}

impl Display for KoboDisplay {
    fn size(&self) -> Size {
        self.rotation.logical_size(self.panel.size)
    }

    fn format(&self) -> PixelFormat {
        self.panel.format
    }

    fn dpi(&self) -> u32 {
        self.panel.dpi
    }

    fn present(&mut self, frame: &Pixmap, refresh: &Refresh) -> Result<(), DisplayError> {
        let screen = self.size();
        check_present(screen, frame, refresh)?;
        if refresh.region.is_empty() {
            return Ok(());
        }

        let region = align_region(refresh.region, screen);
        let plan = plan_refresh(&Refresh { region, ..*refresh }, screen);
        let at = logical_rect_to_panel(region, self.rotation, self.panel.size);

        let logical = crop(frame, region).ok_or_else(|| backend("refresh region does not fit the frame"))?;
        let pixels = rotate_to_panel(&logical, self.rotation);

        self.draw(&pixels, at)?;

        // KOReader drops refreshes of width or height 1 or less (`mxc_update`). The
        // drawing above has already happened, as it does in KOReader.
        if at.w <= 1 || at.h <= 1 {
            return Ok(());
        }

        if plan.wait_before {
            self.wait_complete(fb::LAST_MARKER);
        }
        self.refresh(at, &plan)?;
        let marker = self.last_marker();
        if plan.wait_submission_after {
            self.wait_submission(marker);
        }
        if plan.wait_after {
            self.wait_complete(marker);
        }
        Ok(())
    }

    fn set_rotation(&mut self, rotation: Rotation) -> Result<(), DisplayError> {
        self.rotation = rotation;
        Ok(())
    }

    /// Waits for the last update on the panel. Errors are ignored, as KOReader ignores
    /// them. A wait that fails only costs time.
    fn wait_idle(&mut self) {
        self.wait_complete(fb::LAST_MARKER);
    }
}

/// Copies a rectangle of `frame` into a new pixmap of the same format.
fn crop(frame: &Pixmap, rect: Rect) -> Option<Pixmap> {
    let bpp = frame.format().bytes_per_pixel();
    let row = rect.w as usize * bpp;
    let stride = frame.stride();
    let src = frame.data();
    let mut data = Vec::with_capacity(row * rect.h as usize);
    for y in rect.y..rect.bottom() {
        let start = y as usize * stride + rect.x as usize * bpp;
        data.extend_from_slice(src.get(start..start + row)?);
    }
    Pixmap::from_data(rect.w, rect.h, frame.format(), data)
}

fn backend(msg: impl Into<String>) -> DisplayError {
    DisplayError::Backend(msg.into())
}
