//! Device ioctls: the RTC wakeup alarm, the RTC to system clock copy and the
//! `ntx_io` frontlight.
//!
//! This is the only module in the crate with `unsafe` code. Every other module
//! reaches the kernel through sysfs files and processes. The calls here are
//! ports of KOReader's FFI modules:
//! - `base/ffi/rtc.lua` (`setWakeupAlarm`, `toggleAlarmInterrupt`, `readHardwareClock`, `HCToSys`)
//!   and the structs and request numbers in `base/ffi/rtc_h.lua`;
//! - `base/ffi/kobolight.lua` (`setBrightness`, ioctl 241 on `/dev/ntx_io`).
//!
//! Only Linux has the devices, so on other systems every call returns
//! [`io::ErrorKind::Unsupported`]. The struct layouts, the request numbers and
//! the date arithmetic are plain Rust and run on any host, so the tests check
//! them without a device. Nothing here has run on a Libra Colour yet.

use std::io;
use std::mem::size_of;
use std::path::Path;

/// The ioctl request number for `NTX_SET_BRIGHTNESS`-style frontlight control on
/// `/dev/ntx_io` (`base/ffi/kobolight.lua:31`).
pub const NTX_IO_SET_BRIGHTNESS: u32 = 241;

/// The Linux `rtc_time` struct (`linux/rtc.h`, `base/ffi/rtc_h.lua`). It is the
/// first nine fields of `struct tm`, all `int`, so it is 36 bytes on armv7 and
/// on 64-bit hosts.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RtcTime {
    /// Seconds, 0 to 59.
    pub tm_sec: i32,
    /// Minutes, 0 to 59.
    pub tm_min: i32,
    /// Hours, 0 to 23.
    pub tm_hour: i32,
    /// Day of the month, 1 to 31.
    pub tm_mday: i32,
    /// Month, 0 to 11.
    pub tm_mon: i32,
    /// Years since 1900.
    pub tm_year: i32,
    /// Unused by Linux. KOReader sets -1 on writes.
    pub tm_wday: i32,
    /// Unused by Linux. KOReader sets -1 on writes.
    pub tm_yday: i32,
    /// Unused by Linux. KOReader sets -1 on writes.
    pub tm_isdst: i32,
}

/// The Linux `rtc_wkalrm` struct (`linux/rtc.h`, `base/ffi/rtc_h.lua`): two
/// bytes, padding to 4, then an [`RtcTime`]. 40 bytes on armv7 and on 64-bit
/// hosts.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RtcWkalrm {
    /// 1 when the alarm interrupt is on.
    pub enabled: u8,
    /// Set by the kernel when the alarm has fired and was not read yet.
    pub pending: u8,
    /// The alarm time, UTC.
    pub time: RtcTime,
}

const _: () = assert!(size_of::<RtcTime>() == 36);
const _: () = assert!(size_of::<RtcWkalrm>() == 40);

// The `_IOC` encoding of `asm-generic/ioctl.h`, which armv7 and x86 use: 8 bits of
// number, 8 of type, 14 of size, 2 of direction.
const IOC_WRITE: u32 = 1;
const IOC_READ: u32 = 2;

const fn ioc(dir: u32, kind: u8, nr: u32, size: usize) -> u32 {
    (dir << 30) | ((size as u32) << 16) | ((kind as u32) << 8) | nr
}

/// `RTC_AIE_OFF`, `_IO('p', 0x02)`. Disables the alarm interrupt.
pub const RTC_AIE_OFF: u32 = ioc(0, b'p', 0x02, 0);
/// `RTC_WKALM_SET`, `_IOW('p', 0x0f, struct rtc_wkalrm)`. Sets the alarm and the
/// enable bit in one call, replacing an alarm that is already set.
pub const RTC_WKALM_SET: u32 = ioc(IOC_WRITE, b'p', 0x0f, size_of::<RtcWkalrm>());
/// `RTC_RD_TIME`, `_IOR('p', 0x09, struct rtc_time)`. Reads the clock.
pub const RTC_RD_TIME: u32 = ioc(IOC_READ, b'p', 0x09, size_of::<RtcTime>());

/// Breaks a UTC epoch into an [`RtcTime`] the way `gmtime_r` does, with the unused
/// fields at -1 as KOReader sets them (`rtc.lua:setWakeupAlarm`). `None` when the
/// year does not fit.
pub fn epoch_to_rtc_time(epoch: i64) -> Option<RtcTime> {
    let days = epoch.div_euclid(86_400);
    let secs = epoch.rem_euclid(86_400);
    // Howard Hinnant's civil_from_days.
    let z = days.checked_add(719_468)?;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era.checked_mul(400)? + i64::from(month <= 2);
    Some(RtcTime {
        tm_sec: (secs % 60) as i32,
        tm_min: (secs / 60 % 60) as i32,
        tm_hour: (secs / 3_600) as i32,
        tm_mday: day as i32,
        tm_mon: (month - 1) as i32,
        tm_year: i32::try_from(year - 1_900).ok()?,
        tm_wday: -1,
        tm_yday: -1,
        tm_isdst: -1,
    })
}

/// Turns an [`RtcTime`] in UTC into an epoch, as `timegm` does.
pub fn rtc_time_to_epoch(t: &RtcTime) -> i64 {
    let month = i64::from(t.tm_mon) + 1;
    let year = i64::from(t.tm_year) + 1_900 - i64::from(month <= 2);
    let era = year.div_euclid(400);
    let yoe = year.rem_euclid(400);
    let mp = if month > 2 { month - 3 } else { month + 9 };
    let doy = (153 * mp + 2) / 5 + i64::from(t.tm_mday) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    days * 86_400 + i64::from(t.tm_hour) * 3_600 + i64::from(t.tm_min) * 60 + i64::from(t.tm_sec)
}

/// Sets the RTC alarm to `epoch` (UTC) and enables it, with `RTC_WKALM_SET` on the
/// RTC device node. A set alarm is overwritten, unlike a write to the sysfs
/// `wakealarm` file, which fails with EBUSY. Mirrors `RTC:setWakeupAlarm`.
pub fn rtc_set_alarm(device: &Path, epoch: u64) -> io::Result<()> {
    let epoch = i64::try_from(epoch).map_err(|_| invalid("alarm epoch is out of range"))?;
    let time = epoch_to_rtc_time(epoch).ok_or_else(|| invalid("alarm epoch is out of range"))?;
    let wake = RtcWkalrm {
        enabled: 1,
        pending: 0,
        time,
    };
    imp::rtc_ioctl(device, RTC_WKALM_SET, &wake)
}

/// Turns the RTC alarm interrupt off with `RTC_AIE_OFF`. The alarm time stays in
/// the chip but cannot fire. Mirrors `RTC:unsetWakeupAlarm` (`toggleAlarmInterrupt(false)`).
pub fn rtc_disable_alarm(device: &Path) -> io::Result<()> {
    imp::rtc_aie_off(device)
}

/// Reads the RTC with `RTC_RD_TIME` and returns the epoch (UTC).
pub fn rtc_read_epoch(device: &Path) -> io::Result<i64> {
    let mut time = RtcTime::default();
    imp::rtc_read(device, &mut time)?;
    Ok(rtc_time_to_epoch(&time))
}

/// Sets the system clock to the RTC time. Mirrors `RTC:HCToSys` (`rtc.lua:352`):
/// a first `settimeofday` with a zero zone locks the kernel's `warp_clock`, a
/// second one stores the local UTC offset as the kernel zone (FAT timestamps use
/// it), and the third sets the time. Needs root.
pub fn set_system_clock_from_rtc(device: &Path) -> io::Result<()> {
    let epoch = rtc_read_epoch(device)?;
    imp::set_system_time(epoch)
}

/// Whether the `ntx_io` device node opens.
pub fn ntx_available(device: &Path) -> bool {
    imp::ntx_open(device).is_ok()
}

/// Sets the frontlight brightness (0 to 100) with ioctl 241 on `ntx_io`. Mirrors
/// `kobolight:setBrightness` (`base/ffi/kobolight.lua:30`).
pub fn ntx_set_brightness(device: &Path, value: u8) -> io::Result<()> {
    if value > 100 {
        return Err(invalid("brightness is above 100"));
    }
    imp::ntx_set(device, value)
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

#[cfg(target_os = "linux")]
mod imp {
    #![allow(unsafe_code)]

    use std::fs::{File, OpenOptions};
    use std::io;
    use std::os::fd::AsRawFd;
    use std::os::raw::c_int;
    use std::os::unix::fs::OpenOptionsExt;
    use std::path::Path;
    use std::ptr;

    use super::{RtcTime, RtcWkalrm, NTX_IO_SET_BRIGHTNESS, RTC_AIE_OFF, RTC_RD_TIME};

    /// `struct timezone` of `sys/time.h`. The `libc` crate leaves it opaque.
    #[repr(C)]
    struct Timezone {
        tz_minuteswest: c_int,
        tz_dsttime: c_int,
    }

    /// Opens a device the way KOReader does: read only, non-blocking. `std` adds
    /// `O_CLOEXEC`.
    fn open(path: &Path) -> io::Result<File> {
        OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(path)
    }

    pub(super) fn rtc_ioctl(device: &Path, request: u32, arg: &RtcWkalrm) -> io::Result<()> {
        let file = open(device)?;
        // SAFETY: `file` is open for the call. `arg` points to a live `RtcWkalrm`, the
        // struct that `RTC_WKALM_SET` encodes in its request number (40 bytes, checked by
        // the const assertion in the parent module). The kernel only reads it.
        let rc = unsafe { libc::ioctl(file.as_raw_fd(), request as _, arg as *const RtcWkalrm) };
        check(rc)
    }

    pub(super) fn rtc_aie_off(device: &Path) -> io::Result<()> {
        let file = open(device)?;
        // SAFETY: `file` is open for the call. `RTC_AIE_OFF` takes no argument, and KOReader
        // passes 0. Nothing is read or written through the pointer-sized argument.
        let rc = unsafe { libc::ioctl(file.as_raw_fd(), RTC_AIE_OFF as _, 0) };
        check(rc)
    }

    pub(super) fn rtc_read(device: &Path, out: &mut RtcTime) -> io::Result<()> {
        let file = open(device)?;
        // SAFETY: `file` is open for the call. `out` points to a live, writable `RtcTime`,
        // the 36-byte struct that `RTC_RD_TIME` encodes. The kernel writes exactly that.
        let rc = unsafe { libc::ioctl(file.as_raw_fd(), RTC_RD_TIME as _, out as *mut RtcTime) };
        check(rc)
    }

    pub(super) fn set_system_time(epoch: i64) -> io::Result<()> {
        let secs = libc::time_t::try_from(epoch)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "epoch does not fit time_t"))?;
        let zero = Timezone {
            tz_minuteswest: 0,
            tz_dsttime: 0,
        };
        // SAFETY: a null time value and a pointer to a live, initialized zone struct of the
        // layout `settimeofday` expects. The call only reads the zone.
        check(unsafe { libc::settimeofday(ptr::null(), &zero as *const Timezone as *const libc::timezone) })?;

        // SAFETY: `tm` is zeroed memory of the C type, which is a valid `tm` (the zone name
        // pointer is null). `localtime_r` reads `secs` and writes only `tm`.
        let mut tm: libc::tm = unsafe { std::mem::zeroed() };
        // SAFETY: both pointers refer to live locals for the call.
        if unsafe { libc::localtime_r(&secs, &mut tm) }.is_null() {
            return Err(io::Error::last_os_error());
        }
        let local = Timezone {
            tz_minuteswest: -(tm.tm_gmtoff / 60) as c_int,
            tz_dsttime: 0,
        };
        // SAFETY: as for the first call.
        check(unsafe {
            libc::settimeofday(ptr::null(), &local as *const Timezone as *const libc::timezone)
        })?;

        let tv = libc::timeval {
            tv_sec: secs,
            tv_usec: 0,
        };
        // SAFETY: a pointer to a live `timeval` and a null zone. The call only reads the time.
        check(unsafe { libc::settimeofday(&tv, ptr::null()) })
    }

    pub(super) fn ntx_open(device: &Path) -> io::Result<File> {
        open(device)
    }

    pub(super) fn ntx_set(device: &Path, value: u8) -> io::Result<()> {
        let file = open(device)?;
        // SAFETY: `file` is open for the call. Request 241 takes the brightness by value as
        // an `int`, not a pointer, as `kobolight.lua` passes it. Nothing is dereferenced.
        let rc = unsafe { libc::ioctl(file.as_raw_fd(), NTX_IO_SET_BRIGHTNESS as _, c_int::from(value)) };
        check(rc)
    }

    fn check(rc: c_int) -> io::Result<()> {
        if rc < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
}

#[cfg(not(target_os = "linux"))]
mod imp {
    use std::fs::File;
    use std::io;
    use std::path::Path;

    use super::{RtcTime, RtcWkalrm};

    fn unsupported<T>() -> io::Result<T> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "device ioctls exist only on Linux",
        ))
    }

    pub(super) fn rtc_ioctl(_: &Path, _: u32, _: &RtcWkalrm) -> io::Result<()> {
        unsupported()
    }

    pub(super) fn rtc_aie_off(_: &Path) -> io::Result<()> {
        unsupported()
    }

    pub(super) fn rtc_read(_: &Path, _: &mut RtcTime) -> io::Result<()> {
        unsupported()
    }

    pub(super) fn set_system_time(_: i64) -> io::Result<()> {
        unsupported()
    }

    pub(super) fn ntx_open(_: &Path) -> io::Result<File> {
        unsupported()
    }

    pub(super) fn ntx_set(_: &Path, _: u8) -> io::Result<()> {
        unsupported()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_numbers_match_rtc_h_lua() {
        // base/ffi/rtc_h.lua
        assert_eq!(RTC_AIE_OFF, 28_674);
        assert_eq!(RTC_WKALM_SET, 1_076_391_951);
        assert_eq!(RTC_RD_TIME, 2_149_871_625);
    }

    #[test]
    fn struct_sizes_match_the_kernel() {
        assert_eq!(size_of::<RtcTime>(), 36);
        assert_eq!(size_of::<RtcWkalrm>(), 40);
    }

    #[test]
    fn epoch_breaks_down_like_gmtime() {
        let t = epoch_to_rtc_time(0).unwrap();
        assert_eq!((t.tm_year, t.tm_mon, t.tm_mday), (70, 0, 1));
        assert_eq!((t.tm_hour, t.tm_min, t.tm_sec), (0, 0, 0));
        // 2001-09-09 01:46:40 UTC.
        let t = epoch_to_rtc_time(1_000_000_000).unwrap();
        assert_eq!((t.tm_year, t.tm_mon, t.tm_mday), (101, 8, 9));
        assert_eq!((t.tm_hour, t.tm_min, t.tm_sec), (1, 46, 40));
        // 2000-02-29 12:34:56 UTC, a leap day.
        let t = epoch_to_rtc_time(951_782_400 + 12 * 3_600 + 34 * 60 + 56).unwrap();
        assert_eq!((t.tm_year, t.tm_mon, t.tm_mday), (100, 1, 29));
        assert_eq!((t.tm_hour, t.tm_min, t.tm_sec), (12, 34, 56));
        // The unused fields are -1, as KOReader sets them.
        assert_eq!((t.tm_wday, t.tm_yday, t.tm_isdst), (-1, -1, -1));
    }

    #[test]
    fn epoch_round_trips_through_rtc_time() {
        for epoch in [
            0,
            1,
            86_399,
            951_782_400,
            1_000_000_000,
            1_788_000_000,
            4_102_444_800,
        ] {
            let t = epoch_to_rtc_time(epoch).unwrap();
            assert_eq!(rtc_time_to_epoch(&t), epoch, "epoch {epoch}");
        }
    }

    #[test]
    fn out_of_range_alarm_epochs_are_rejected() {
        let err = rtc_set_alarm(Path::new("/nonexistent"), u64::MAX).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    }

    #[test]
    fn missing_device_nodes_fail_cleanly() {
        let missing = Path::new("/nonexistent/rtc0");
        assert!(rtc_set_alarm(missing, 1_000).is_err());
        assert!(rtc_disable_alarm(missing).is_err());
        assert!(rtc_read_epoch(missing).is_err());
        assert!(set_system_clock_from_rtc(missing).is_err());
        assert!(!ntx_available(missing));
        assert!(ntx_set_brightness(missing, 10).is_err());
    }

    #[test]
    fn brightness_above_100_is_rejected_before_the_device_opens() {
        let err = ntx_set_brightness(Path::new("/nonexistent"), 101).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    }
}
