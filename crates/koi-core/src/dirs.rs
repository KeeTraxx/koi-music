//! Application directories (`ch.compile.koi-music`).

use std::path::PathBuf;

use crate::error::{Error, ErrorKind, Result};

pub const APP_ID: &str = "ch.compile.koi-music";
pub const APP_NAME: &str = "koi-music";

#[derive(Debug, Clone)]
pub struct AppDirs {
    /// Database and rustypipe cache.
    pub data: PathBuf,
    /// Stream buffers and other disposable files.
    pub cache: PathBuf,
    /// Default folder for downloaded tracks.
    pub default_downloads: PathBuf,
}

impl AppDirs {
    pub fn discover() -> Result<Self> {
        let project = directories::ProjectDirs::from("ch", "compile", APP_NAME)
            .ok_or_else(|| Error::new(ErrorKind::Io, "could not determine home directory"))?;
        let data = project.data_dir().to_path_buf();
        let default_downloads = dirs::audio_dir()
            .map(|d| d.join(APP_NAME))
            .unwrap_or_else(|| data.join("downloads"));
        Ok(Self { data, cache: project.cache_dir().to_path_buf(), default_downloads })
    }

    /// Directories rooted at `base`, for tests.
    pub fn in_dir(base: impl Into<PathBuf>) -> Self {
        let base = base.into();
        Self { data: base.join("data"), cache: base.join("cache"), default_downloads: base.join("downloads") }
    }

    pub fn create_all(&self) -> Result<()> {
        std::fs::create_dir_all(&self.data)?;
        std::fs::create_dir_all(&self.cache)?;
        Ok(())
    }

    pub fn database(&self) -> PathBuf {
        self.data.join("library.db")
    }

    pub fn rustypipe_storage(&self) -> PathBuf {
        self.data.join("rustypipe")
    }
}
