//! Fonts and icons shared by every window.
//!
//! One [`Assets`] value holds the font system and the icon cache. Windows share
//! it through [`SharedAssets`], so fonts load once per process.

use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use kaia_gfx::image::{render_svg, SvgError};
use kaia_gfx::text::{FontSystem, TextError, NOTO_FONT_PREFIXES};
use kaia_gfx::{AlphaPixmap, Color, Size};

/// The assets a window needs while it lays out and paints. Single-threaded.
pub type SharedAssets = Rc<RefCell<Assets>>;

/// Errors while loading assets.
#[derive(Debug)]
pub enum AssetError {
    Fonts(TextError),
    IconDir { path: PathBuf, source: io::Error },
}

impl fmt::Display for AssetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AssetError::Fonts(e) => write!(f, "fonts: {e}"),
            AssetError::IconDir { path, source } => {
                write!(f, "cannot read icon directory {}: {source}", path.display())
            }
        }
    }
}

impl std::error::Error for AssetError {}

/// Why an icon could not be drawn.
#[derive(Debug)]
pub enum IconError {
    /// The name contains a path separator or is empty.
    BadName(String),
    Read {
        path: PathBuf,
        source: io::Error,
    },
    Render(SvgError),
}

impl fmt::Display for IconError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IconError::BadName(name) => write!(f, "bad icon name {name:?}"),
            IconError::Read { path, source } => write!(f, "cannot read {}: {source}", path.display()),
            IconError::Render(e) => write!(f, "cannot render icon: {e}"),
        }
    }
}

impl std::error::Error for IconError {}

/// Fonts and icons. Build one per process with [`Assets::load`].
pub struct Assets {
    pub fonts: FontSystem,
    pub icons: IconCache,
}

impl Assets {
    /// Loads the Noto fonts from `fonts_dir` and points the icon cache at
    /// `icons_dir` (the `mdlight` set, one `<name>.svg` per icon).
    pub fn load(fonts_dir: &Path, icons_dir: &Path) -> Result<Self, AssetError> {
        let fonts = FontSystem::load(&[fonts_dir], NOTO_FONT_PREFIXES).map_err(AssetError::Fonts)?;
        fs::read_dir(icons_dir).map_err(|source| AssetError::IconDir {
            path: icons_dir.to_path_buf(),
            source,
        })?;
        Ok(Self {
            fonts,
            icons: IconCache::new(icons_dir.to_path_buf()),
        })
    }

    /// Wraps the result of [`Assets::load`] for sharing between windows.
    pub fn shared(self) -> SharedAssets {
        Rc::new(RefCell::new(self))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct IconKey {
    name: String,
    w: u32,
    h: u32,
    rgb: [u8; 3],
}

/// Rendered icons, keyed by name, size and color. Each entry is a tinted
/// alpha mask, so a repeated draw does no SVG work.
pub struct IconCache {
    dir: PathBuf,
    cache: HashMap<IconKey, Rc<AlphaPixmap>>,
}

impl IconCache {
    pub fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            cache: HashMap::new(),
        }
    }

    /// The icon `name` at `size`, drawn in `color`. The SVG alpha channel
    /// gives the shape, so icons must be monochrome.
    pub fn get(&mut self, name: &str, size: Size, color: Color) -> Result<Rc<AlphaPixmap>, IconError> {
        if name.is_empty() || name.contains('/') || name.contains('\\') {
            return Err(IconError::BadName(name.to_string()));
        }
        let key = IconKey {
            name: name.to_string(),
            w: size.w,
            h: size.h,
            rgb: [color.r, color.g, color.b],
        };
        if let Some(hit) = self.cache.get(&key) {
            return Ok(Rc::clone(hit));
        }
        let path = self.dir.join(format!("{name}.svg"));
        let svg = fs::read(&path).map_err(|source| IconError::Read {
            path: path.clone(),
            source,
        })?;
        let shape = render_svg(&svg, size).map_err(IconError::Render)?;
        let tinted = Rc::new(tint(&shape, color));
        self.cache.insert(key, Rc::clone(&tinted));
        Ok(tinted)
    }

    /// Number of cached icon masks.
    pub fn len(&self) -> usize {
        self.cache.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cache.is_empty()
    }
}

/// Keeps the shape (alpha) of `shape` and paints it in `color`.
fn tint(shape: &AlphaPixmap, color: Color) -> AlphaPixmap {
    let mut out = AlphaPixmap::new(shape.width(), shape.height());
    for y in 0..shape.height() as i32 {
        for x in 0..shape.width() as i32 {
            if let Some([_, _, _, a]) = shape.get(x, y) {
                out.put(x, y, [color.r, color.g, color.b, a]);
            }
        }
    }
    out
}
