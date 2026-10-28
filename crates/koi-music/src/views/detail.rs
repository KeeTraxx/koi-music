//! Album, remote playlist and artist pages.

use eframe::egui::{self, RichText, Ui};
use koi_core::import::RemoteCollection;
use koi_core::model::{AlbumDetail, ArtistDetail, PlaylistDetail, Track};
use koi_core::ytm::playlist_all_tracks;

use crate::app::{Action, Ctx, Target};
use crate::fetch::{Load, Spawner};
use crate::widgets::{self, RowOpts};

/// What a detail page shows.
pub enum Page {
    Album { id: String, load: Load<AlbumDetail> },
    Playlist { id: String, load: Load<(PlaylistDetail, Vec<Track>)> },
    Artist { id: String, load: Load<ArtistDetail> },
}

impl Page {
    pub fn open(target: &Target, sp: &Spawner, cx: &Ctx) -> Option<Self> {
        let mut page = match target {
            Target::Album(id) => Self::Album { id: id.clone(), load: Load::Idle },
            Target::RemotePlaylist(id) => Self::Playlist { id: id.clone(), load: Load::Idle },
            Target::Artist(id) => Self::Artist { id: id.clone(), load: Load::Idle },
            _ => return None,
        };
        page.fetch(sp, cx);
        Some(page)
    }

    fn fetch(&mut self, sp: &Spawner, cx: &Ctx) {
        let src = cx.core.source.clone();
        match self {
            Self::Album { id, load } => {
                let id = id.clone();
                *load = sp.load(async move { src.album(&id).await });
            }
            Self::Playlist { id, load } => {
                let id = id.clone();
                *load = sp.load(async move { playlist_all_tracks(src.as_ref(), &id).await });
            }
            Self::Artist { id, load } => {
                let id = id.clone();
                *load = sp.load(async move { src.artist(&id).await });
            }
        }
    }

    pub fn ui(&mut self, ui: &mut Ui, sp: &Spawner, cx: &Ctx, out: &mut Vec<Action>) {
        let mut retry = false;
        match self {
            Self::Album { load, .. } => {
                load.poll();
                match load {
                    Load::Ready(a) => album_ui(ui, cx, a, out),
                    Load::Failed(e) => retry = widgets::error_box(ui, "Loading the album", e),
                    _ => widgets::loading(ui),
                }
            }
            Self::Playlist { load, .. } => {
                load.poll();
                match load {
                    Load::Ready((p, tracks)) => playlist_ui(ui, cx, p, tracks, out),
                    Load::Failed(e) => retry = widgets::error_box(ui, "Loading the playlist", e),
                    _ => widgets::loading(ui),
                }
            }
            Self::Artist { load, .. } => {
                load.poll();
                match load {
                    Load::Ready(a) => artist_ui(ui, cx, a, out),
                    Load::Failed(e) => retry = widgets::error_box(ui, "Loading the artist", e),
                    _ => widgets::loading(ui),
                }
            }
        }
        if retry {
            self.fetch(sp, cx);
        }
    }
}

fn header(ui: &mut Ui, thumb: Option<&str>, title: &str, subtitle: &str, buttons: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        widgets::thumb(ui, thumb, 160.0);
        ui.vertical(|ui| {
            ui.add_space(8.0);
            ui.label(RichText::new(title).heading().strong());
            ui.label(RichText::new(subtitle).weak());
            ui.add_space(12.0);
            ui.horizontal_wrapped(buttons);
        });
    });
    ui.separator();
}

fn track_list(ui: &mut Ui, cx: &Ctx, tracks: &[Track], out: &mut Vec<Action>) {
    egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        for (i, t) in tracks.iter().enumerate() {
            widgets::track_row(ui, cx, t, tracks, i, RowOpts { number: Some(i + 1), ..Default::default() }, out);
        }
    });
}

fn album_ui(ui: &mut Ui, cx: &Ctx, a: &AlbumDetail, out: &mut Vec<Action>) {
    let artists: Vec<&str> = a.artists.iter().map(|x| x.name.as_str()).collect();
    let mut sub = format!("Album · {}", artists.join(", "));
    if let Some(y) = a.year {
        sub.push_str(&format!(" · {y}"));
    }
    sub.push_str(&format!(" · {} tracks", a.tracks.len()));
    header(ui, a.thumbnail.as_deref(), &a.title, &sub, |ui| {
        widgets::collection_buttons(ui, cx, &a.tracks, out);
        if ui.button("💾 Save as playlist").clicked() {
            out.push(Action::SaveAsPlaylist(RemoteCollection::Album(a.id.clone())));
        }
        if let Some(id) = a.artists.iter().find_map(|x| x.id.clone())
            && ui.button("👤 Artist").clicked() {
                out.push(Action::Open(Target::Artist(id)));
            }
    });
    track_list(ui, cx, &a.tracks, out);
}

fn playlist_ui(ui: &mut Ui, cx: &Ctx, p: &PlaylistDetail, tracks: &[Track], out: &mut Vec<Action>) {
    let mut sub = "Playlist".to_owned();
    if let Some(o) = &p.owner {
        sub.push_str(&format!(" · {o}"));
    }
    sub.push_str(&format!(" · {} tracks", tracks.len()));
    header(ui, p.thumbnail.as_deref(), &p.title, &sub, |ui| {
        widgets::collection_buttons(ui, cx, tracks, out);
        if ui.button("💾 Save as playlist").clicked() {
            out.push(Action::SaveAsPlaylist(RemoteCollection::Playlist(p.id.clone())));
        }
    });
    track_list(ui, cx, tracks, out);
}

fn artist_ui(ui: &mut Ui, cx: &Ctx, a: &ArtistDetail, out: &mut Vec<Action>) {
    header(ui, a.thumbnail.as_deref(), &a.name, "Artist", |ui| {
        widgets::collection_buttons(ui, cx, &a.top_tracks, out);
    });
    egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        if !a.top_tracks.is_empty() {
            ui.label(RichText::new("Top songs").heading());
            for (i, t) in a.top_tracks.iter().enumerate() {
                widgets::track_row(ui, cx, t, &a.top_tracks, i, RowOpts::default(), out);
            }
        }
        if !a.albums.is_empty() {
            ui.add_space(12.0);
            ui.label(RichText::new("Albums").heading());
            ui.horizontal_wrapped(|ui| {
                for album in &a.albums {
                    let card = ui.vertical(|ui| {
                        ui.set_width(140.0);
                        widgets::thumb(ui, album.thumbnail.as_deref(), 140.0);
                        ui.add(egui::Label::new(&album.title).truncate().selectable(false));
                        if let Some(y) = album.year {
                            ui.label(RichText::new(y.to_string()).weak().small());
                        }
                    });
                    let resp = ui.interact(card.response.rect, ui.scope_id().with(("album", &album.id)), egui::Sense::click());
                    if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                        out.push(Action::Open(Target::Album(album.id.clone())));
                    }
                }
            });
        }
    });
}
