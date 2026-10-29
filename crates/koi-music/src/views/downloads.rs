//! Downloads: queued, running and failed jobs, and the downloaded tracks.

use std::path::Path;

use eframe::egui::{self, RichText, Ui};
use koi_core::downloads::{Job, JobState};
use koi_core::model::Track;

use crate::app::{Action, Ctx};
use crate::widgets::{self, RowOpts};

pub fn downloads_ui(ui: &mut Ui, cx: &Ctx, out: &mut Vec<Action>) {
    let jobs = cx.core.downloads.state();
    ui.horizontal(|ui| {
        ui.label(RichText::new("Downloads").heading().strong());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let dir = cx.core.downloads.dir();
            if ui.button("📂 Open Directory").on_hover_text(dir.display().to_string()).clicked()
                && let Err(e) = open_in_file_manager(&dir) {
                    out.push(Action::Toast(format!("Could not open {}: {e}", dir.display())));
                }
        });
    });
    ui.label(RichText::new(format!("Folder: {}", cx.core.downloads.dir().display())).weak().small());
    ui.separator();

    egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        if !jobs.jobs.is_empty() {
            ui.label(RichText::new("Queue").strong());
            for job in &jobs.jobs {
                job_row(ui, cx, job);
            }
            ui.add_space(12.0);
        }

        ui.label(RichText::new("Downloaded tracks").strong());
        let mut tracks: Vec<Track> = cx.lib.downloads.values().map(|d| d.track.clone()).collect();
        tracks.sort_by(|a, b| a.artist_line().cmp(&b.artist_line()).then_with(|| a.title.cmp(&b.title)));
        if tracks.is_empty() {
            ui.label(RichText::new("Nothing downloaded yet. Use “Download” on a song, album or playlist.").weak());
        }
        for (i, t) in tracks.iter().enumerate() {
            ui.horizontal(|ui| {
                if ui.small_button("🗑").on_hover_text("Delete the file").clicked() {
                    out.push(Action::DeleteDownload(t.video_id.clone()));
                }
                widgets::track_row(ui, cx, t, &tracks, i, RowOpts::default(), out);
            });
        }
    });
}

fn job_row(ui: &mut Ui, cx: &Ctx, job: &Job) {
    ui.horizontal(|ui| {
        widgets::thumb(ui, job.track.thumbnail.as_deref(), 32.0);
        // Button first (right-aligned); the details use the remaining width.
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if job.state.is_active() {
                if ui.button("Cancel").clicked() {
                    cx.core.downloads.cancel(job.id);
                }
            } else if matches!(job.state, JobState::Failed(_) | JobState::Cancelled) && ui.button("Retry").clicked() {
                cx.core.downloads.retry(job.id);
            }
            ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                ui.add(egui::Label::new(format!("{} — {}", job.track.artist_line(), job.track.title)).truncate());
                match &job.state {
                    JobState::Queued => {
                        ui.label(RichText::new("Queued").weak().small());
                    }
                    JobState::Downloading { .. } => {
                        let f = job.state.fraction().unwrap_or(0.0);
                        ui.add(egui::ProgressBar::new(f).show_percentage().desired_height(12.0));
                    }
                    JobState::Failed(e) => {
                        let msg = RichText::new(format!("Failed: {e}")).small();
                        ui.add(egui::Label::new(msg.color(ui.visuals().error_fg_color)).truncate())
                            .on_hover_text(e.as_str());
                    }
                    JobState::Cancelled => {
                        ui.label(RichText::new("Cancelled").weak().small());
                    }
                }
            });
        });
    });
}

/// Show `dir` in the OS file manager, creating it first (it only exists after the first download).
fn open_in_file_manager(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    #[cfg(windows)]
    let program = "explorer";
    #[cfg(target_os = "macos")]
    let program = "open";
    #[cfg(not(any(windows, target_os = "macos")))]
    let program = "xdg-open";
    let mut child = std::process::Command::new(program).arg(dir).spawn()?;
    // Reap the launcher; its exit code is not meaningful (explorer.exe returns 1 on success).
    std::thread::spawn(move || child.wait());
    Ok(())
}

/// Short status for the navigation panel, e.g. "2 active".
pub fn nav_badge(cx: &Ctx) -> Option<String> {
    let active = cx.core.downloads.state().jobs.iter().filter(|j| j.state.is_active()).count();
    (active > 0).then(|| format!("{active} active"))
}
