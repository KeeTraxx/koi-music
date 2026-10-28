//! Local library pages: a local playlist and "Recently played".

use eframe::egui::{self, Id, RichText, Ui};
use egui_dnd::DragDropItem;
use koi_core::library::{PlaylistEntry, PlaylistId};
use koi_core::model::Track;

use crate::app::{Action, Ctx};
use crate::widgets::{self, RowOpts};

/// A playlist entry in the drag-and-drop list, identified by its stable entry id.
struct DndEntry<'a> {
    entry: &'a PlaylistEntry,
    index: usize,
}

impl DragDropItem for DndEntry<'_> {
    fn id(&self) -> Id {
        Id::unique(("playlist-entry", self.entry.id))
    }
}

/// Convert egui_dnd's insertion index (before removal) to the final index.
pub fn dnd_target(from: usize, to: usize) -> usize {
    if to > from { to - 1 } else { to }
}

pub fn playlist_ui(ui: &mut Ui, cx: &Ctx, id: PlaylistId, out: &mut Vec<Action>) {
    let Some(p) = cx.lib.playlist(id) else {
        ui.label(RichText::new("This playlist no longer exists.").weak());
        return;
    };
    let tracks: Vec<Track> = p.tracks();
    ui.horizontal(|ui| {
        widgets::thumb(ui, tracks.first().and_then(|t| t.thumbnail.as_deref()), 120.0);
        ui.vertical(|ui| {
            ui.add_space(8.0);
            ui.label(RichText::new(&p.name).heading().strong());
            let n = tracks.len();
            let mut sub = format!("Playlist · {n} track{}", if n == 1 { "" } else { "s" });
            if p.source_id.is_some() {
                sub.push_str(" · saved from YouTube Music");
            }
            ui.label(RichText::new(sub).weak());
            ui.add_space(12.0);
            ui.horizontal_wrapped(|ui| {
                widgets::collection_buttons(ui, cx, &tracks, out);
                if ui.button("✏ Rename").clicked() {
                    out.push(Action::RenamePlaylist(id));
                }
                if ui.button("🗑 Delete").clicked() {
                    out.push(Action::DeletePlaylist(id));
                }
            });
        });
    });
    ui.separator();
    if tracks.is_empty() {
        ui.label(RichText::new("No tracks yet. Use “Add to playlist” on any song.").weak());
        return;
    }

    egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        let items = p.entries.iter().enumerate().map(|(index, entry)| DndEntry { entry, index });
        let response = egui_dnd::dnd(ui, Id::unique(("playlist-dnd", id))).show(items, |ui, item, handle, _state| {
            ui.horizontal(|ui| {
                widgets::drag_handle(ui, handle);
                let entry_id = item.entry.id;
                let opts = RowOpts {
                    number: Some(item.index + 1),
                    extra_menu: Some(Box::new(move |ui: &mut Ui, out: &mut Vec<Action>| {
                        if ui.button("✖ Remove from playlist").clicked() {
                            out.push(Action::RemoveFromPlaylist { playlist: id, entry: entry_id });
                        }
                    })),
                };
                widgets::track_row(ui, cx, &item.entry.track, &tracks, item.index, opts, out);
            });
        });
        if let Some(update) = response.final_update() {
            let to = dnd_target(update.from, update.to);
            if to != update.from {
                out.push(Action::MovePlaylistEntry { playlist: id, from: update.from, to });
            }
        }
    });
}

pub fn history_ui(ui: &mut Ui, cx: &Ctx, out: &mut Vec<Action>) {
    ui.label(RichText::new("Recently played").heading().strong());
    ui.separator();
    if cx.lib.history.is_empty() {
        ui.label(RichText::new("Songs you listen to for at least 30 seconds show up here.").weak());
        return;
    }
    let tracks: Vec<Track> = cx.lib.history.iter().map(|h| h.track.clone()).collect();
    egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        for (i, t) in tracks.iter().enumerate() {
            widgets::track_row(ui, cx, t, &tracks, i, RowOpts::default(), out);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::dnd_target;

    #[test]
    fn dnd_insertion_index_to_final_index() {
        // Third item dragged to the top.
        assert_eq!(dnd_target(2, 0), 0);
        // First item dropped after the third (insertion index 3).
        assert_eq!(dnd_target(0, 3), 2);
    }
}
