//! Core library of koi-music: YouTube Music access, playback, library and downloads.
//!
//! Everything here is independent of the UI so it can be tested headlessly.

pub mod app;
pub mod audio;
pub mod dirs;
pub mod downloads;
pub mod error;
pub mod fsname;
pub mod http_reader;
pub mod import;
pub mod library;
pub mod media;
pub mod model;
pub mod notify;
pub mod player;
pub mod queue;
pub mod ytdlp;
pub mod ytm;

pub use error::{Error, ErrorKind, Result};
