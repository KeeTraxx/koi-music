//! Settings: download folder, concurrency, yt-dlp path and status.

use std::path::PathBuf;

use eframe::egui::{self, RichText, Ui};
use koi_core::app::ToolProbe;

use crate::app::{Action, Ctx};

#[derive(Default)]
pub struct SettingsView {
    download_dir: Option<String>,
    ytdlp_path: Option<String>,
}

impl SettingsView {
    pub fn ui(&mut self, ui: &mut Ui, cx: &Ctx, out: &mut Vec<Action>) {
        let core = cx.core;
        ui.label(RichText::new("Settings").heading().strong());
        ui.separator();

        egui::Grid::new("settings").num_columns(2).spacing([16.0, 12.0]).show(ui, |ui| {
            ui.label("Download folder");
            ui.horizontal(|ui| {
                let dir = self.download_dir.get_or_insert_with(|| core.downloads.dir().display().to_string());
                ui.add(egui::TextEdit::singleline(dir).desired_width(360.0));
                if ui.button("Apply").clicked() {
                    let path = PathBuf::from(dir.trim());
                    match core.set_download_dir(path) {
                        Ok(()) => out.push(Action::Toast("Download folder changed".into())),
                        Err(e) => out.push(Action::Toast(e.to_string())),
                    }
                }
            });
            ui.end_row();

            ui.label("Simultaneous downloads");
            let mut n = core.downloads.concurrency();
            if ui.add(egui::Slider::new(&mut n, 1..=10)).changed()
                && let Err(e) = core.set_download_concurrency(n) {
                    out.push(Action::Toast(e.to_string()));
                }
            ui.end_row();

            ui.label("yt-dlp");
            ui.horizontal(|ui| {
                let path = self.ytdlp_path.get_or_insert_with(|| core.ytdlp.bin().display().to_string());
                ui.add(egui::TextEdit::singleline(path).desired_width(360.0).hint_text("yt-dlp"));
                if ui.button("Apply").clicked() {
                    let p = if path.trim().is_empty() { "yt-dlp" } else { path.trim() };
                    if let Err(e) = core.set_ytdlp_path(PathBuf::from(p)) {
                        out.push(Action::Toast(e.to_string()));
                    }
                }
                if ui.button("Check again").clicked() {
                    core.probe_tool();
                }
            });
            ui.end_row();

            ui.label("");
            tool_status(ui, &core.tool());
            ui.end_row();
        });

        ui.add_space(16.0);
        ui.label(
            RichText::new(
                "yt-dlp and a JavaScript runtime (Deno recommended) are not shipped with koi-music. \
                 Install them yourself and keep yt-dlp up to date. See the README.",
            )
            .weak()
            .small(),
        );
        if let Some(e) = &core.audio_error {
            ui.colored_label(ui.visuals().error_fg_color, format!("Audio output unavailable: {e}"));
        }
    }
}

pub fn tool_status(ui: &mut Ui, tool: &ToolProbe) {
    match tool {
        ToolProbe::Pending => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Checking yt-dlp…");
            });
        }
        ToolProbe::Ready(s) => {
            ui.vertical(|ui| {
                ui.label(format!("✔ yt-dlp {}", s.version));
                if s.js_runtimes.is_empty() {
                    ui.colored_label(ui.visuals().warn_fg_color, "⚠ No JavaScript runtime found (install Deno)");
                } else {
                    ui.label(RichText::new(format!("JS runtime: {}", s.js_runtimes.join(", "))).weak());
                }
            });
        }
        ToolProbe::Missing(e) => {
            ui.colored_label(ui.visuals().error_fg_color, format!("✖ {e}"));
        }
    }
}
