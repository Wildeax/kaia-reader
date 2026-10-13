//! Golden image checks for headless render tests.
//!
//! A golden file is a PNG kept in the repository. A test renders a pixmap and
//! compares it with the golden file. On a mismatch the test writes the new
//! render next to the golden file as `<name>.actual.png` and fails. Run the
//! tests with `KAIA_UPDATE_GOLDEN=1` to write the golden files from the
//! current output.

use std::fs;
use std::path::{Path, PathBuf};

use crate::pixmap::{PixelFormat, Pixmap};
use crate::png::write_pixmap;

/// Environment variable that turns comparisons into golden file updates.
pub const UPDATE_ENV: &str = "KAIA_UPDATE_GOLDEN";

/// Compares `pixmap` with the PNG at `path`. Panics on a mismatch.
///
/// `tolerance` is the largest difference allowed in any gray or RGB sample,
/// from 0 (exact) to 255. Dimensions and pixel format must match exactly.
///
/// With `KAIA_UPDATE_GOLDEN=1` the golden file is written from `pixmap` and the
/// check passes. Missing parent directories are created.
pub fn assert_matches_golden(pixmap: &Pixmap, path: &Path, tolerance: u8) {
    let update = std::env::var(UPDATE_ENV).is_ok_and(|v| v == "1");
    check(pixmap, path, tolerance, update);
}

fn check(pixmap: &Pixmap, path: &Path, tolerance: u8, update: bool) {
    let actual = actual_path(path);
    if update {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).unwrap_or_else(|e| panic!("cannot create {}: {e}", dir.display()));
        }
        write_pixmap(pixmap, path).unwrap_or_else(|e| panic!("cannot write {}: {e}", path.display()));
        let _ = fs::remove_file(&actual);
        return;
    }

    let expected = match load_golden(pixmap.format(), path) {
        Ok(p) => p,
        Err(reason) => panic!(
            "golden file {} unusable: {reason}. Run with {UPDATE_ENV}=1 to create it.",
            path.display()
        ),
    };
    match compare(pixmap, &expected, tolerance) {
        None => {
            let _ = fs::remove_file(&actual);
        }
        Some(report) => {
            write_pixmap(pixmap, &actual)
                .unwrap_or_else(|e| panic!("cannot write {}: {e}", actual.display()));
            panic!(
                "{} does not match its golden file: {report}. Actual render written to {}",
                path.display(),
                actual.display()
            );
        }
    }
}

/// `sample.png` becomes `sample.actual.png` in the same directory.
fn actual_path(path: &Path) -> PathBuf {
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    path.with_file_name(format!("{stem}.actual.png"))
}

fn load_golden(format: PixelFormat, path: &Path) -> Result<Pixmap, String> {
    let img = ::image::ImageReader::open(path)
        .map_err(|e| e.to_string())?
        .decode()
        .map_err(|e| e.to_string())?;
    let (width, height) = (img.width(), img.height());
    match format {
        PixelFormat::Gray8 => {
            if !matches!(img.color(), ::image::ColorType::L8) {
                return Err(format!("color type {:?}, expected 8-bit gray", img.color()));
            }
            Pixmap::from_data(width, height, format, img.to_luma8().into_raw())
                .ok_or_else(|| "decoded size mismatch".to_string())
        }
        PixelFormat::Rgb24 => {
            if !matches!(img.color(), ::image::ColorType::Rgb8) {
                return Err(format!("color type {:?}, expected 8-bit RGB", img.color()));
            }
            Pixmap::from_data(width, height, format, img.to_rgb8().into_raw())
                .ok_or_else(|| "decoded size mismatch".to_string())
        }
    }
}

/// Returns None when the pixmaps match within `tolerance`, or a report of the
/// first problem found.
fn compare(actual: &Pixmap, expected: &Pixmap, tolerance: u8) -> Option<String> {
    if actual.size() != expected.size() {
        return Some(format!(
            "size is {}x{}, golden is {}x{}",
            actual.width(),
            actual.height(),
            expected.width(),
            expected.height()
        ));
    }
    let mut worst = 0u8;
    let mut bad = 0usize;
    for (a, e) in actual.data().iter().zip(expected.data()) {
        let d = a.abs_diff(*e);
        worst = worst.max(d);
        if d > tolerance {
            bad += 1;
        }
    }
    (bad > 0).then(|| format!("{bad} samples differ by more than {tolerance} (largest difference {worst})"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pixmap::Color;

    fn scratch_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("kaia-gfx-golden-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn pattern(v: u8) -> Pixmap {
        let mut pm = Pixmap::new(4, 4, PixelFormat::Gray8);
        pm.put(1, 1, Color::gray(v));
        pm
    }

    #[test]
    fn matching_render_passes_and_writes_nothing() {
        let dir = scratch_dir("match");
        let path = dir.join("g.png");
        check(&pattern(10), &path, 0, true);
        check(&pattern(10), &path, 0, false);
        assert!(!actual_path(&path).exists());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn tolerance_allows_small_differences() {
        let dir = scratch_dir("tolerance");
        let path = dir.join("g.png");
        check(&pattern(10), &path, 0, true);
        check(&pattern(13), &path, 3, false);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn mismatch_writes_actual_and_panics() {
        let dir = scratch_dir("mismatch");
        let path = dir.join("g.png");
        check(&pattern(10), &path, 0, true);
        let result = std::panic::catch_unwind(|| check(&pattern(200), &path, 0, false));
        assert!(result.is_err(), "mismatch must panic");
        let actual = dir.join("g.actual.png");
        assert!(actual.exists(), "actual render written next to the golden file");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn missing_golden_panics_with_hint() {
        let dir = scratch_dir("missing");
        let path = dir.join("absent.png");
        let result = std::panic::catch_unwind(|| check(&pattern(0), &path, 0, false));
        assert!(result.is_err());
        fs::remove_dir_all(&dir).unwrap();
    }
}
