//! Shared widgets: thumbnails, the track row with its context menu, error boxes.

use std::time::Duration;

use eframe::egui::{self, Color32, RichText, Sense, Ui, Vec2};
use koi_core::model::Track;

use crate::app::{Action, Ctx, Target};

pub fn fmt_duration(d: Duration) -> String {
    let s = d.as_secs();
    if s >= 3600 { format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60) } else { format!("{}:{:02}", s / 60, s % 60) }
}

pub fn fmt_secs(s: Option<u32>) -> String {
    s.map(|s| fmt_duration(Duration::from_secs(s.into()))).unwrap_or_default()
}

/// A square thumbnail loaded from a URL (or a placeholder).
pub fn thumb(ui: &mut Ui, url: Option<&str>, size: f32) -> egui::Response {
    match url {
        Some(url) => ui.add(
            egui::Image::new(url.to_owned())
                .fit_to_exact_size(Vec2::splat(size))
                .maintain_aspect_ratio(true)
                .corner_radius(4.0),
        ),
        None => {
            let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
            ui.painter().rect_filled(rect, 4.0, ui.visuals().faint_bg_color);
            ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, "♪", egui::FontId::proportional(size * 0.4), ui.visuals().weak_text_color());
            resp
        }
    }
}

/// Shows a request error with a Retry button. Returns true if Retry was clicked.
pub fn error_box(ui: &mut Ui, what: &str, err: &koi_core::Error) -> bool {
    let mut retry = false;
    egui::Frame::group(ui.style()).fill(ui.visuals().extreme_bg_color).show(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.colored_label(ui.visuals().error_fg_color, format!("⚠ {what} failed: {err}"));
            retry = ui.button("Retry").clicked();
        });
    });
    retry
}

pub fn loading(ui: &mut Ui) {
    ui.horizontal(|ui| {
        ui.spinner();
        ui.label("Loading…");
    });
}

/// The grip used to drag-reorder rows in the queue and in playlists.
pub fn drag_handle(ui: &mut Ui, handle: egui_dnd::Handle) {
    handle.ui(ui, |ui| {
        ui.label("☰").on_hover_cursor(egui::CursorIcon::Grab).on_hover_text("Drag to reorder");
    });
}

/// Extra context-menu entries for a track row.
pub type ExtraMenu = Box<dyn FnOnce(&mut Ui, &mut Vec<Action>)>;

/// Height of a [`track_row`] (its thumbnail size).
const ROW_HEIGHT: f32 = 40.0;

/// Options for [`track_row`].
#[derive(Default)]
pub struct RowOpts {
    /// Position number shown on the left (1-based).
    pub number: Option<usize>,
    /// Extra entries at the end of the context menu (e.g. "Remove from playlist").
    pub extra_menu: Option<ExtraMenu>,
}

/// One track: thumbnail, title, artist, duration, downloaded badge.
/// Clicking plays `list` from `index`; right-click (or "⋯") opens the track menu.
pub fn track_row(ui: &mut Ui, cx: &Ctx, track: &Track, list: &[Track], index: usize, opts: RowOpts, out: &mut Vec<Action>) {
    let playing = cx.player.current_track().is_some_and(|t| t.video_id == track.video_id);
    let downloaded = cx.lib.is_downloaded(&track.video_id);
    let playable = downloaded || cx.can_stream;
    // The row's click area is registered before its contents so the "…" button on top
    // of it gets its own clicks (later widgets win on overlap).
    let row_rect = egui::Rect::from_min_size(ui.cursor().min, Vec2::new(ui.available_width(), ROW_HEIGHT));
    let resp = ui.interact(row_rect, ui.scope_id().with(("track_row", &track.video_id, index)), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(row_rect, 4.0, ui.visuals().widgets.hovered.weak_bg_fill.gamma_multiply(0.3));
    }
    ui.horizontal(|ui| {
        ui.set_min_height(ROW_HEIGHT);
        if let Some(n) = opts.number {
            ui.add_sized([24.0, ROW_HEIGHT], egui::Label::new(RichText::new(n.to_string()).weak()));
        }
        thumb(ui, track.thumbnail.as_deref(), ROW_HEIGHT);
        // Right-hand widgets first; the text then truncates to the remaining width.
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let menu = ui.menu_button("…", |ui| track_menu(ui, cx, track, list, index, playable, downloaded, out)).response;
            menu.on_hover_text("More");
            ui.label(fmt_secs(track.duration_s));
            if downloaded {
                ui.label(RichText::new("⬇").color(Color32::from_rgb(80, 170, 90))).on_hover_text("Downloaded");
            }
            ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                let mut title = RichText::new(&track.title);
                if playing {
                    title = title.strong().color(ui.visuals().selection.stroke.color);
                }
                ui.add(egui::Label::new(title).truncate().selectable(false));
                let mut sub = track.artist_line();
                if let Some(a) = &track.album {
                    sub = format!("{sub} · {}", a.name);
                }
                ui.add(egui::Label::new(RichText::new(sub).weak().small()).truncate().selectable(false));
            });
        });
    });
    if resp.clicked() && playable {
        out.push(Action::Play { tracks: list.to_vec(), start: index });
    }
    let extra = opts.extra_menu;
    resp.context_menu(|ui| {
        track_menu(ui, cx, track, list, index, playable, downloaded, out);
        if let Some(extra) = extra {
            ui.separator();
            extra(ui, out);
        }
    });
}

#[allow(clippy::too_many_arguments)]
fn track_menu(ui: &mut Ui, cx: &Ctx, track: &Track, list: &[Track], index: usize, playable: bool, downloaded: bool, out: &mut Vec<Action>) {
    if ui.add_enabled(playable, egui::Button::new("⏵ Play")).clicked() {
        out.push(Action::Play { tracks: list.to_vec(), start: index });
    }
    if ui.add_enabled(playable, egui::Button::new("⤵ Play next")).clicked() {
        out.push(Action::PlayNext(track.clone()));
    }
    if ui.add_enabled(playable, egui::Button::new("➕ Add to queue")).clicked() {
        out.push(Action::Enqueue(vec![track.clone()]));
    }
    add_to_playlist_menu(ui, cx, std::slice::from_ref(track), out);
    if downloaded {
        if ui.button("🗑 Delete download").clicked() {
            out.push(Action::DeleteDownload(track.video_id.clone()));
        }
    } else if ui.add_enabled(cx.can_stream, egui::Button::new("⬇ Download")).clicked() {
        out.push(Action::Download(vec![track.clone()]));
    }
    if let Some(album) = &track.album
        && ui.button("💿 Go to album").clicked() {
            out.push(Action::Open(Target::Album(album.id.clone())));
        }
    if let Some(id) = track.artists.iter().find_map(|a| a.id.clone())
        && ui.button("👤 Go to artist").clicked() {
            out.push(Action::Open(Target::Artist(id)));
        }
}

/// "Add to playlist" submenu listing local playlists plus "New playlist…".
pub fn add_to_playlist_menu(ui: &mut Ui, cx: &Ctx, tracks: &[Track], out: &mut Vec<Action>) {
    ui.menu_button("☰ Add to playlist", |ui| {
        for p in &cx.lib.playlists {
            if ui.button(&p.name).clicked() {
                out.push(Action::AddToPlaylist { playlist: p.id, tracks: tracks.to_vec(), confirmed: false });
            }
        }
        if !cx.lib.playlists.is_empty() {
            ui.separator();
        }
        if ui.button("New playlist…").clicked() {
            out.push(Action::NewPlaylist { with: tracks.to_vec() });
        }
    });
}

/// Play / Shuffle / Queue / Download-all buttons for a list of tracks.
pub fn collection_buttons(ui: &mut Ui, cx: &Ctx, tracks: &[Track], out: &mut Vec<Action>) {
    let any = !tracks.is_empty();
    if ui.add_enabled(any, egui::Button::new("⏵ Play")).clicked() {
        out.push(Action::Play { tracks: tracks.to_vec(), start: 0 });
    }
    if ui.add_enabled(any, egui::Button::new("🔀 Shuffle")).clicked() {
        out.push(Action::PlayShuffled(tracks.to_vec()));
    }
    if ui.add_enabled(any, egui::Button::new("➕ Add to queue")).clicked() {
        out.push(Action::Enqueue(tracks.to_vec()));
    }
    let missing = tracks.iter().filter(|t| !cx.lib.is_downloaded(&t.video_id)).count();
    let label = if missing == 0 && any { "✔ Downloaded".to_owned() } else { format!("⬇ Download all ({missing})") };
    if ui.add_enabled(any && missing > 0 && cx.can_stream, egui::Button::new(label)).clicked() {
        out.push(Action::Download(tracks.to_vec()));
    }
}
