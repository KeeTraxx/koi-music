//! Error type shared by all core services.

use std::fmt;

/// Coarse error categories the UI can react to (e.g. offer "Retry" for `Network`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ErrorKind {
    Network,
    Unavailable,
    NotFound,
    Db,
    Io,
    Cancelled,
    MissingTool,
    /// YouTube asked to "confirm you're not a bot".
    BotCheck,
    Invalid,
    Audio,
}

#[derive(Debug, Clone, thiserror::Error)]
pub struct Error {
    pub kind: ErrorKind,
    pub message: String,
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let prefix = match self.kind {
            ErrorKind::Network => "Network error",
            ErrorKind::Unavailable => "Unavailable",
            ErrorKind::NotFound => "Not found",
            ErrorKind::Db => "Database error",
            ErrorKind::Io => "File error",
            ErrorKind::Cancelled => "Cancelled",
            ErrorKind::MissingTool => "Required tool not found",
            ErrorKind::BotCheck => "YouTube bot check",
            ErrorKind::Invalid => "Invalid input",
            ErrorKind::Audio => "Audio error",
        };
        if self.message.is_empty() {
            f.write_str(prefix)
        } else {
            write!(f, "{prefix}: {}", self.message)
        }
    }
}

impl Error {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self { kind, message: message.into() }
    }

    pub fn network(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Network, message)
    }

    pub fn unavailable(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Unavailable, message)
    }

    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Invalid, message)
    }

    pub fn audio(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Audio, message)
    }

    pub fn cancelled() -> Self {
        Self::new(ErrorKind::Cancelled, "")
    }

    pub fn is_retryable(&self) -> bool {
        self.kind == ErrorKind::Network
    }
}

impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        Self::new(ErrorKind::Db, e.to_string())
    }
}

impl From<rusqlite_migration::Error> for Error {
    fn from(e: rusqlite_migration::Error) -> Self {
        Self::new(ErrorKind::Db, e.to_string())
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self::new(ErrorKind::Io, e.to_string())
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Self::new(ErrorKind::Db, e.to_string())
    }
}

impl From<rustypipe::error::Error> for Error {
    fn from(e: rustypipe::error::Error) -> Self {
        use rustypipe::error::{Error as RpError, ExtractionError};
        let kind = match &e {
            RpError::Http(_) | RpError::HttpStatus(..) => ErrorKind::Network,
            RpError::Extraction(ExtractionError::NotFound { .. }) => ErrorKind::NotFound,
            RpError::Extraction(ExtractionError::Unavailable { .. }) => ErrorKind::Unavailable,
            RpError::Extraction(_) | RpError::Auth(_) | RpError::Other(_) => ErrorKind::Unavailable,
        };
        Self::new(kind, e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_per_kind() {
        let cases = [
            (ErrorKind::Network, "Network error: offline"),
            (ErrorKind::Unavailable, "Unavailable: offline"),
            (ErrorKind::NotFound, "Not found: offline"),
            (ErrorKind::Db, "Database error: offline"),
            (ErrorKind::Io, "File error: offline"),
            (ErrorKind::Cancelled, "Cancelled: offline"),
            (ErrorKind::MissingTool, "Required tool not found: offline"),
            (ErrorKind::BotCheck, "YouTube bot check: offline"),
            (ErrorKind::Invalid, "Invalid input: offline"),
            (ErrorKind::Audio, "Audio error: offline"),
        ];
        for (kind, expected) in cases {
            assert_eq!(Error::new(kind, "offline").to_string(), expected);
        }
        assert_eq!(Error::cancelled().to_string(), "Cancelled");
    }

    #[test]
    fn only_network_is_retryable() {
        assert!(Error::network("x").is_retryable());
        assert!(!Error::unavailable("x").is_retryable());
    }
}
