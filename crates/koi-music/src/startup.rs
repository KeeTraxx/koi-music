//! Logging and fatal-error reporting, so a failed start is never silent (design D10).
//!
//! Windows release builds have no console, so errors also go to a log file and, for
//! fatal errors, to a message box.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use tracing_subscriber::prelude::*;
use tracing_subscriber::{EnvFilter, fmt};

pub const LOG_FILE_NAME: &str = "koi-music.log";

/// Log to stderr and to `<log_dir>/koi-music.log` (replacing the previous run's log).
/// Returns the log file path if it could be created.
pub fn init_logging(log_dir: Option<&Path>) -> Option<PathBuf> {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| "info,rustypipe=warn".into());
    let log_path = log_dir.map(|d| d.join(LOG_FILE_NAME));
    let file = log_path.as_ref().and_then(|p| {
        std::fs::create_dir_all(p.parent()?).ok()?;
        File::create(p).ok()
    });
    let file_layer = file.map(|f| fmt::layer().with_ansi(false).with_writer(Mutex::new(f)));
    tracing_subscriber::registry().with(filter).with(fmt::layer().with_writer(std::io::stderr)).with(file_layer).init();
    log_path.filter(|p| p.is_file())
}

/// Log panics (and show them on Windows) before the default hook runs.
pub fn install_panic_hook(log_path: Option<PathBuf>) {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        report_fatal(&format!("koi-music crashed: {info}"), log_path.as_deref());
        default_hook(info);
    }));
}

/// Log a fatal error and, on Windows, show it in a message box that names the log file.
pub fn report_fatal(message: &str, log_path: Option<&Path>) {
    tracing::error!("{message}");
    let text = match log_path {
        Some(p) => format!("{message}\n\nDetails are in the log file:\n{}", p.display()),
        None => message.to_owned(),
    };
    show_error_dialog(&text);
}

#[cfg(windows)]
fn show_error_dialog(text: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};
    let wide = |s: &str| s.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
    let (text, caption) = (wide(text), wide("koi-music"));
    // SAFETY: both strings are NUL-terminated UTF-16 and outlive the call; no owner window.
    unsafe {
        MessageBoxW(std::ptr::null_mut(), text.as_ptr(), caption.as_ptr(), MB_OK | MB_ICONERROR);
    }
}

#[cfg(not(windows))]
fn show_error_dialog(_text: &str) {
    // Linux runs with a terminal or a session log; stderr already has the error.
}
