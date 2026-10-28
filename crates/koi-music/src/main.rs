//! koi-music: a YouTube Music desktop player.

mod app;
mod fetch;
mod views;
mod widgets;

use eframe::egui;
use koi_core::app::AppCore;
use koi_core::dirs::{APP_ID, APP_NAME, AppDirs};
use koi_core::notify::Notifier;

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
            koi_core::media::start(core.player.clone(), &handle);
            Ok(Box::new(app::KoiMusicApp::new(core, ctx)))
        }),
    )
}
