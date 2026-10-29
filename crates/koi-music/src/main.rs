//! koi-music: a YouTube Music desktop player.

// Windows release builds are GUI apps without a console window; debug builds keep
// the console so `cargo run` shows logs.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod fetch;
mod views;
mod widgets;

use eframe::egui;
use koi_core::app::AppCore;
use koi_core::dirs::{APP_ID, APP_NAME, AppDirs};
use koi_core::notify::Notifier;

/// The Win32 window handle, needed for the Windows media controls.
#[cfg(windows)]
fn window_handle(cc: &eframe::CreationContext<'_>) -> Option<*mut std::ffi::c_void> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    match cc.window_handle().ok()?.as_raw() {
        RawWindowHandle::Win32(h) => Some(h.hwnd.get() as *mut std::ffi::c_void),
        _ => None,
    }
}

#[cfg(not(windows))]
fn window_handle(_cc: &eframe::CreationContext<'_>) -> Option<*mut std::ffi::c_void> {
    None
}

fn main() -> eframe::Result {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info,rustypipe=warn".into()),
        )
        .init();

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("koi-music-rt")
        .build()
        .expect("cannot start tokio runtime");
    // Lets UI code spawn tasks (e.g. download cancellation) from the main thread.
    let _guard = rt.enter();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_app_id(APP_ID)
            .with_title("koi-music")
            .with_inner_size([1200.0, 800.0])
            .with_min_inner_size([820.0, 520.0]),
        ..Default::default()
    };
    let handle = rt.handle().clone();
    eframe::run_native(
        APP_NAME,
        options,
        Box::new(move |cc| {
            egui_extras::install_image_loaders(&cc.egui_ctx);
            let ctx = cc.egui_ctx.clone();
            let repaint = ctx.clone();
            let core = AppCore::start(handle.clone(), AppDirs::discover()?, Notifier::new(move || repaint.request_repaint()))?;
            let media = koi_core::media::MediaSession::new(core.player.clone(), window_handle(cc));
            Ok(Box::new(app::KoiMusicApp::new(core, ctx, media)))
        }),
    )
}
