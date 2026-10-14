use std::ffi::{c_char, CString};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::{check, ffi, path_cstring, Result, DATA_DIR, FONTS_DIR, KAIA_FONTS_DIR};

/// The user's font folder. Its fonts join the built-in folders in every
/// [`RuntimeOptions`] built after the call to [`set_user_font_dir`].
static USER_FONT_DIR: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Sets the folder that holds the user's fonts, or clears it with None. Kaia
/// sets it once at start, from its data folder. A missing folder is fine.
pub fn set_user_font_dir(dir: Option<PathBuf>) {
    *USER_FONT_DIR.lock().unwrap_or_else(|p| p.into_inner()) = dir;
}

/// The folder set by [`set_user_font_dir`].
pub fn user_font_dir() -> Option<PathBuf> {
    USER_FONT_DIR.lock().unwrap_or_else(|p| p.into_inner()).clone()
}

/// CREngine resources chosen at runtime. Relative paths use the current directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeOptions {
    /// Folder containing epub.css, fb2.css and the hyph subfolder.
    pub data_dir: PathBuf,
    /// Font folders searched recursively, excluding KOReader's MuPDF fallbacks.
    /// The user font folder comes last when it is set.
    pub font_dirs: Vec<PathBuf>,
}

impl Default for RuntimeOptions {
    fn default() -> Self {
        let install = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(Path::to_path_buf));
        Self::from_install_dir(install.as_deref().unwrap_or(Path::new(".")))
    }
}

impl RuntimeOptions {
    /// Uses resources beside the executable when present, otherwise the desktop
    /// build tree. The desktop tree adds the repository's Kaia reading fonts.
    /// An explicit options value never falls back to another folder.
    pub fn from_install_dir(install: &Path) -> Self {
        let installed = install.join("data").is_dir() || install.join("fonts").is_dir();
        let (data_dir, mut font_dirs) = if installed {
            (install.join("data"), vec![install.join("fonts")])
        } else {
            (DATA_DIR.into(), vec![FONTS_DIR.into(), KAIA_FONTS_DIR.into()])
        };
        font_dirs.extend(user_font_dir());
        Self { data_dir, font_dirs }
    }

    /// Lists font files in a stable order. Skips symlinked folders to avoid loops.
    pub fn font_files(&self) -> Vec<PathBuf> {
        let mut out = Vec::new();
        let mut stack = self.font_dirs.clone();
        while let Some(dir) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let Ok(kind) = entry.file_type() else { continue };
                let path = entry.path();
                if kind.is_dir() {
                    if entry.file_name() != "urw" {
                        stack.push(path);
                    }
                    continue;
                }
                let is_font = path
                    .extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "ttf" | "otf" | "ttc"));
                let symbols = entry.file_name() == "symbols.ttf" && dir.ends_with("nerdfonts");
                if is_font && !symbols {
                    out.push(path);
                }
            }
        }
        out.sort();
        out.dedup();
        out
    }
}

/// Registers the runtime fonts and selects <data_dir>/hyph. A failed call can
/// be retried with corrected paths. The engine shares fonts across documents.
pub fn init_with_options(options: &RuntimeOptions) -> Result<usize> {
    let owned: Vec<CString> = options
        .font_files()
        .iter()
        .map(|p| path_cstring(p))
        .collect::<Result<_>>()?;
    let ptrs: Vec<*const c_char> = owned.iter().map(|c| c.as_ptr()).collect();
    let data = path_cstring(&options.data_dir)?;
    let mut registered = 0;
    // SAFETY: the strings and pointer array outlive the call; the output is writable.
    check(unsafe {
        ffi::kaia_cre_init_with_data(ptrs.as_ptr(), ptrs.len(), data.as_ptr(), &mut registered)
    })?;
    Ok(registered.max(0) as usize)
}
