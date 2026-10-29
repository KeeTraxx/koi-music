//! koi-music: a YouTube Music desktop player.

// Windows release builds are GUI apps without a console window; debug builds keep
// the console so `cargo run` shows logs.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod fetch;
mod startup;
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

/// Windows renders with wgpu (Direct3D 12), choosing a real GPU when there is one and
/// Microsoft's WARP software renderer otherwise, e.g. in VMs with a display-only driver
/// (design D9). Linux keeps the glow (OpenGL) renderer.
#[cfg(windows)]
fn configure_renderer(options: &mut eframe::NativeOptions) {
    use eframe::egui_wgpu::WgpuSetup;
    options.renderer = eframe::Renderer::Wgpu;
    if let WgpuSetup::CreateNew(setup) = &mut options.wgpu_options.wgpu_setup {
        setup.native_adapter_selector = Some(std::sync::Arc::new(select_adapter));
    }
}

#[cfg(not(windows))]
fn configure_renderer(_options: &mut eframe::NativeOptions) {}

/// Prefer hardware adapters, fall back to software (WARP); only adapters that can draw to the window.
#[cfg(windows)]
fn select_adapter(
    adapters: &[eframe::wgpu::Adapter],
    surface: Option<&eframe::wgpu::Surface<'_>>,
) -> Result<eframe::wgpu::Adapter, String> {
    use eframe::wgpu::DeviceType;
    let rank = |t: DeviceType| match t {
        DeviceType::DiscreteGpu => 0,
        DeviceType::IntegratedGpu => 1,
        DeviceType::VirtualGpu => 2,
        DeviceType::Other => 3,
        DeviceType::Cpu => 4,
    };
    let describe = |a: &eframe::wgpu::Adapter| {
        let i = a.get_info();
        format!("{} ({:?}, {:?})", i.name, i.device_type, i.backend)
    };
    tracing::info!("graphics adapters: {}", adapters.iter().map(describe).collect::<Vec<_>>().join("; "));
    let chosen = adapters
        .iter()
        .filter(|a| surface.is_none_or(|s| a.is_surface_supported(s)))
        .min_by_key(|a| rank(a.get_info().device_type))
        .ok_or_else(|| format!("no graphics adapter can draw to the window (found {})", adapters.len()))?;
    tracing::info!("using graphics adapter: {}", describe(chosen));
    Ok(chosen.clone())
}

fn main() -> eframe::Result {
    let dirs = AppDirs::discover();
    let log_path = startup::init_logging(dirs.as_ref().ok().map(|d| d.cache.as_path()));
    startup::install_panic_hook(log_path.clone());
    tracing::info!(
        "koi-music {} starting on {} ({}), log file: {}",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
        log_path.as_ref().map_or_else(|| "none".to_owned(), |p| p.display().to_string())
    );
    let dirs = match dirs {
        Ok(d) => d,
        Err(e) => {
            startup::report_fatal(&format!("koi-music could not start: {e}"), log_path.as_deref());
            std::process::exit(1);
        }
    };

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("koi-music-rt")
        .build()
        .expect("cannot start tokio runtime");
    // Lets UI code spawn tasks (e.g. download cancellation) from the main thread.
    let _guard = rt.enter();

    let mut options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_app_id(APP_ID)
            .with_title("koi-music")
            .with_inner_size([1200.0, 800.0])
            .with_min_inner_size([820.0, 520.0]),
        ..Default::default()
    };
    configure_renderer(&mut options);
    let handle = rt.handle().clone();
    let result = eframe::run_native(
        APP_NAME,
        options,
        Box::new(move |cc| {
            egui_extras::install_image_loaders(&cc.egui_ctx);
            let ctx = cc.egui_ctx.clone();
            let repaint = ctx.clone();
            let core = AppCore::start(handle.clone(), dirs, Notifier::new(move || repaint.request_repaint()))?;
            let media = koi_core::media::MediaSession::new(core.player.clone(), window_handle(cc));
            Ok(Box::new(app::KoiMusicApp::new(core, ctx, media)))
        }),
    );
    if let Err(e) = &result {
        startup::report_fatal(&format!("koi-music could not start: {e}"), log_path.as_deref());
    }
    result
}
