//! Directories the Lua host reads and writes.

use std::path::{Path, PathBuf};

use crate::Error;

/// The directories of a Lua host.
///
/// `koreader_dir` is the installed KOReader tree, the `koreader/` folder that
/// `./kodev build` creates. It holds `common/`, `ffi/`, `frontend/`,
/// `plugins/`, `libs/` and `l10n/`, and the root Lua files such as
/// `datastorage.lua` and `defaults.lua`. The host uses this layout, as
/// `setupkoenv.lua` does, so it needs no `base/` directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LuaPaths {
    /// Kaia's Lua shims, searched before KOReader's modules. In the repository
    /// this is `rust/kaia-lua/lua`. It ships next to the binary later.
    pub shim_dir: PathBuf,
    /// The KOReader tree (the installed `koreader/` layout).
    pub koreader_dir: PathBuf,
    /// Kaia's data folder. The reader settings and the book sidecars go here.
    pub data_dir: PathBuf,
}

impl LuaPaths {
    pub fn new(
        shim_dir: impl Into<PathBuf>,
        koreader_dir: impl Into<PathBuf>,
        data_dir: impl Into<PathBuf>,
    ) -> Self {
        Self {
            shim_dir: shim_dir.into(),
            koreader_dir: koreader_dir.into(),
            data_dir: data_dir.into(),
        }
    }

    /// KOReader's `frontend/`.
    pub fn frontend_dir(&self) -> PathBuf {
        self.koreader_dir.join("frontend")
    }

    /// KOReader's `plugins/`.
    pub fn plugins_dir(&self) -> PathBuf {
        self.koreader_dir.join("plugins")
    }

    /// KOReader's `common/`, with the Lua libraries that do not come from KOReader.
    pub fn common_dir(&self) -> PathBuf {
        self.koreader_dir.join("common")
    }

    /// KOReader's `libs/`, the native modules that `require("libs/...")` and `ffi.load` read.
    pub fn libs_dir(&self) -> PathBuf {
        self.koreader_dir.join("libs")
    }

    /// The translations, `l10n/<lang>/koreader.mo`.
    pub fn l10n_dir(&self) -> PathBuf {
        self.koreader_dir.join("l10n")
    }

    /// Checks that the directories the host needs exist.
    pub(crate) fn check(&self) -> Result<(), Error> {
        require_file(
            &self.shim_dir.join("datastorage.lua"),
            "Kaia shim datastorage.lua",
        )?;
        require_file(
            &self.koreader_dir.join("setupkoenv.lua"),
            "KOReader setupkoenv.lua",
        )?;
        require_file(
            &self.frontend_dir().join("util.lua"),
            "KOReader frontend/util.lua",
        )?;
        require_dir(&self.libs_dir(), "KOReader libs/")?;
        Ok(())
    }
}

fn require_file(path: &Path, what: &str) -> Result<(), Error> {
    if path.is_file() {
        Ok(())
    } else {
        Err(Error::Init(format!("{what} not found at {}", path.display())))
    }
}

fn require_dir(path: &Path, what: &str) -> Result<(), Error> {
    if path.is_dir() {
        Ok(())
    } else {
        Err(Error::Init(format!("{what} not found at {}", path.display())))
    }
}
