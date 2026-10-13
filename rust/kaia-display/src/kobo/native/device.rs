//! The native Kobo panel: `/dev/fb0` mapped with mmap, and the HWTCON ioctls. Linux only.
//!
//! The steps come from [`Sequencer`](super::update::Sequencer), which holds the logic.
//! This file only moves bytes and issues the calls. Errors on waits and procfs writes
//! are ignored, as KOReader ignores them. A failed update request is reported.

use std::fs::OpenOptions;
use std::io::{self, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::raw::c_void;
use std::ptr;

use kaia_gfx::{PixelFormat, Pixmap, Size};

use super::abi::{
    FbFixScreeninfo, FbVarScreeninfo, HwtconUpdateData, HwtconUpdateMarkerData, FBIOGET_FSCREENINFO,
    FBIOGET_VSCREENINFO, HWTCON_SEND_UPDATE, HWTCON_WAIT_FOR_UPDATE_COMPLETE,
    HWTCON_WAIT_FOR_UPDATE_SUBMISSION,
};
use super::fb::{self, FbGeometry};
use super::geometry::{bounded_rect, Orientation, ALIGNMENT_CONSTRAINT};
use super::pixels::{encode_patch, store_patch, Patch};
use super::update::{request_for, Sequencer, Step};
use crate::display::{check_present, Display, DisplayError, PanelSpec};
use crate::rotation::Rotation;
use crate::waveform::Refresh;

/// The procfs knob for the MTK panel (`device.lua`, `kobo_mtk_wakeup_epdc` and the
/// night mode writes).
const HWTCON_CMD: &str = "/proc/hwtcon/cmd";

/// A Kobo MediaTek panel, driven through the framebuffer and the HWTCON ioctls.
///
/// Opening it maps the framebuffer and holds it until the value is dropped. The
/// framebuffer must be in its native portrait orientation at the panel size. Rotation
/// is done in software, as KOReader does it on Kobo, so the framebuffer is never
/// rotated.
pub struct NativeKoboDisplay {
    /// The framebuffer device, open until drop.
    fd: OwnedFd,
    /// The start of the mapping. It is `map_len` bytes long until drop.
    map: *mut u8,
    map_len: usize,
    geometry: FbGeometry,
    panel: PanelSpec,
    rotation: Rotation,
    sequencer: Sequencer,
}

impl NativeKoboDisplay {
    /// Opens `/dev/fb0`, maps it and checks it against the panel.
    ///
    /// Fails when the framebuffer cannot be opened or mapped, or when its geometry is not
    /// what [`fb::validate`] accepts for `panel`.
    pub fn open(panel: PanelSpec) -> Result<Self, DisplayError> {
        // SAFETY: `c"/dev/fb0"` is a NUL-terminated string that lives for the whole
        // program. `open` reads it and returns a descriptor or -1.
        let raw = unsafe { libc::open(c"/dev/fb0".as_ptr(), libc::O_RDWR | libc::O_CLOEXEC) };
        if raw < 0 {
            return Err(backend(format!(
                "cannot open /dev/fb0: {}",
                io::Error::last_os_error()
            )));
        }
        // SAFETY: `raw` is a fresh descriptor that nothing else owns, so `OwnedFd` may
        // close it once. It is closed when `fd` is dropped, including on the error paths
        // below.
        let fd = unsafe { OwnedFd::from_raw_fd(raw) };

        let mut fix = FbFixScreeninfo::default();
        ioctl_into(&fd, FBIOGET_FSCREENINFO, &mut fix)
            .map_err(|e| backend(format!("FBIOGET_FSCREENINFO failed: {e}")))?;
        let mut var = FbVarScreeninfo::default();
        ioctl_into(&fd, FBIOGET_VSCREENINFO, &mut var)
            .map_err(|e| backend(format!("FBIOGET_VSCREENINFO failed: {e}")))?;
        let geometry = fb::validate(&fix, &var, panel.size).map_err(backend)?;

        // SAFETY: a null hint lets the kernel choose the address. The length is the
        // page-aligned byte count of the screen. `fb::validate` keeps the unaligned
        // size within the reported memory. The rounding up to a page is what KOReader
        // maps too (`PAGE_ALIGN`). The mapping is shared and read-write, and the fd is
        // open for the call. The result is checked against MAP_FAILED below.
        let map = unsafe {
            libc::mmap(
                ptr::null_mut(),
                geometry.map_len,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                fd.as_raw_fd(),
                0,
            )
        };
        if map == libc::MAP_FAILED {
            return Err(backend(format!(
                "cannot map the framebuffer: {}",
                io::Error::last_os_error()
            )));
        }

        let dev = Self {
            fd,
            map: map as *mut u8,
            map_len: geometry.map_len,
            geometry,
            panel,
            rotation: Rotation::Deg0,
            sequencer: Sequencer::default(),
        };

        // KOReader fills the screen with white when it opens the framebuffer
        // (`framebuffer:reinit`, `self.bb:fill(BB.COLOR_WHITE)`). White is 0xFF in every
        // byte of both layouts.
        // SAFETY: `dev.map` is valid for `dev.map_len` bytes until `dev` is dropped, and
        // `fb_size` is at most `map_len`. The write stays inside the mapping. Nothing
        // else references the memory during the call.
        unsafe { ptr::write_bytes(dev.map, 0xFF, dev.geometry.fb_size) };
        Ok(dev)
    }

    /// Opens the Libra Colour panel (1264 by 1680 pixels, 300 dpi).
    pub fn libra_colour() -> Result<Self, DisplayError> {
        Self::open(PanelSpec::LIBRA_COLOUR)
    }

    /// The current rotation. It is set by [`Display::set_rotation`].
    pub fn rotation(&self) -> Rotation {
        self.rotation
    }

    /// True while the night mode is on.
    pub fn night_mode(&self) -> bool {
        self.sequencer.night_mode()
    }

    /// Copies a patch into the mapped framebuffer.
    fn store(&mut self, patch: &Patch) -> Result<(), DisplayError> {
        // SAFETY: `self.map` is valid for `self.map_len` bytes until drop. `&mut self`
        // makes this the only Rust access, and the slice ends at `map_len`.
        let fb = unsafe { std::slice::from_raw_parts_mut(self.map, self.map_len) };
        if store_patch(fb, self.geometry.line_length, patch) {
            Ok(())
        } else {
            Err(backend("patch lies outside the framebuffer"))
        }
    }

    /// Runs planned steps in order. Only a failed update request is an error. The
    /// steps after it still run, as they do in KOReader.
    fn run(&self, steps: &[Step]) -> Result<(), DisplayError> {
        let mut result = Ok(());
        for step in steps {
            match *step {
                Step::Poweron => write_procfs(HWTCON_CMD, "fiti_power 1"),
                Step::WaitComplete(marker) => {
                    let mut data = HwtconUpdateMarkerData {
                        update_marker: marker,
                        collision_test: 0,
                    };
                    let _ = ioctl_into(&self.fd, HWTCON_WAIT_FOR_UPDATE_COMPLETE, &mut data);
                }
                Step::WaitSubmission(marker) => {
                    let mut value = marker;
                    let _ = ioctl_into(&self.fd, HWTCON_WAIT_FOR_UPDATE_SUBMISSION, &mut value);
                }
                Step::Send(update) => {
                    let mut data: HwtconUpdateData = update;
                    if let Err(e) = ioctl_into(&self.fd, HWTCON_SEND_UPDATE, &mut data) {
                        if result.is_ok() {
                            result = Err(backend(format!("HWTCON_SEND_UPDATE failed: {e}")));
                        }
                    }
                }
            }
        }
        result
    }
}

impl Drop for NativeKoboDisplay {
    fn drop(&mut self) {
        // SAFETY: `self.map` and `self.map_len` come from the successful `mmap` in
        // `open`, and this is the only place that unmaps them. After this the fields
        // are not used, and the descriptor closes when `fd` is dropped.
        unsafe {
            libc::munmap(self.map as *mut c_void, self.map_len);
        }
    }
}

impl Display for NativeKoboDisplay {
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
        let orientation = Orientation::new(self.panel.size, self.rotation);
        let screen = orientation.logical_size();
        check_present(screen, frame, refresh)?;
        if refresh.region.is_empty() {
            return Ok(());
        }

        // KOReader bounds and aligns the region, then paints it, then sends the update.
        // The pixels go in first, as KOReader paints before it refreshes.
        let bounded = bounded_rect(refresh.region, screen, Some(ALIGNMENT_CONSTRAINT));
        if let Some(patch) = encode_patch(frame, bounded, &orientation, self.geometry.pixels) {
            self.store(&patch)?;
        }

        let request = request_for(refresh.waveform, refresh.flash, refresh.dither);
        if let Some(steps) = self.sequencer.plan_update(&orientation, bounded, request) {
            self.run(&steps)?;
        }
        Ok(())
    }

    fn set_rotation(&mut self, rotation: Rotation) -> Result<(), DisplayError> {
        self.rotation = rotation;
        Ok(())
    }

    /// Waits until the last update has finished on the panel. Failed waits are ignored.
    fn wait_idle(&mut self) {
        let steps = self.sequencer.plan_wait_for_last();
        // Only the update request reports an error, and waits never do.
        let _ = self.run(&steps);
    }

    /// Turns the night mode on or off. The panel inverts in hardware: the knob
    /// `night_mode 4` or `night_mode 0` is written to `/proc/hwtcon/cmd`, as KOReader does
    /// on MTK (`device.lua`, `setHWNightmode`). Night mode also changes the waveforms
    /// that updates use.
    fn set_night_mode(&mut self, on: bool) {
        self.sequencer.set_night_mode(on);
        write_procfs(HWTCON_CMD, if on { "night_mode 4" } else { "night_mode 0" });
    }
}

/// Issues an ioctl whose argument is the struct `arg` points to. The kernel reads or
/// writes the struct, as the request number says.
fn ioctl_into<T>(fd: &OwnedFd, request: u32, arg: &mut T) -> io::Result<()> {
    // SAFETY: `arg` points to a live `T` for the whole call. The caller passes one of
    // the structs in `abi.rs`, whose size the request number encodes, so the kernel
    // stays inside it. `fd` is open because `OwnedFd` has not been dropped.
    let rc = unsafe { libc::ioctl(fd.as_raw_fd(), request as _, arg as *mut T as *mut c_void) };
    if rc < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Writes `value` to a procfs or sysfs knob without truncating it, and ignores
/// failures. KOReader's `writeToSysfs` (`base/ffi/util.lua`) works the same way.
fn write_procfs(path: &str, value: &str) {
    if let Ok(mut file) = OpenOptions::new().write(true).open(path) {
        let _ = file.write_all(value.as_bytes());
    }
}

fn backend(msg: impl Into<String>) -> DisplayError {
    DisplayError::Backend(msg.into())
}
