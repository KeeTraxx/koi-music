//! Search: query box with debounced suggestions, filter tabs, grouped or paginated results.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use eframe::egui::{self, RichText, Ui};
use koi_core::model::{
    AlbumSummary, ArtistSummary, Continuation, Page, PlaylistSummary, SearchFilter, SearchItem, SearchResults, Track,
};
use koi_core::Error;

use crate::app::{Action, Ctx, Target};
use crate::fetch::{Load, Spawner};
use crate::widgets::{self, RowOpts};

const SUGGEST_DEBOUNCE: Duration = Duration::from_millis(300);

enum Response {
    Grouped(SearchResults),
    Filtered(Page<SearchItem>),
}

/// Results of a filtered search, accumulated over pages.
struct FilteredList {
    items: Vec<SearchItem>,
    seen: HashSet<String>,
    next: Option<Continuation>,
}

impl FilteredList {
    fn new() -> Self {
        Self { items: Vec::new(), seen: HashSet::new(), next: None }
    }

    /// Append a page, skipping items already shown.
    fn extend(&mut self, page: Page<SearchItem>) {
        for item in page.items {
            if self.seen.insert(item_key(&item)) {
                self.items.push(item);
            }
        }
        self.next = page.next;
    }
}

fn item_key(item: &SearchItem) -> String {
    match item {
        SearchItem::Song(t) => format!("s:{}", t.video_id),
        SearchItem::Album(a) => format!("a:{}", a.id),
        SearchItem::Artist(a) => format!("r:{}", a.id),
        SearchItem::Playlist(p) => format!("p:{}", p.id),
    }
}

#[derive(Default)]
pub struct SearchView {
    query: String,
    filter: Option<SearchFilter>,
    /// Last submitted query and filter.
    active: Option<(String, Option<SearchFilter>)>,
    grouped: Option<SearchResults>,
    filtered: Option<FilteredList>,
    request: Load<Response>,
    more: Load<Page<SearchItem>>,
    /// Last failure (the previous results stay visible).
    error: Option<(Error, bool)>,
    suggestions: Vec<String>,
    suggest: Load<Vec<String>>,
    suggested_for: String,
    last_edit: Option<Instant>,
    focus_query: bool,
    /// The pointer was over the suggestions popup last frame (keeps it open while clicking).
    popup_hovered: bool,
}

impl SearchView {
    pub fn new() -> Self {
        Self { focus_query: true, ..Default::default() }
    }

    /// Start a search for the current query. Whitespace-only queries do nothing.
    fn submit(&mut self, sp: &Spawner, cx: &Ctx) {
        let q = self.query.trim().to_owned();
        if q.is_empty() {
            return;
        }
        self.active = Some((q.clone(), self.filter));
        self.suggestions.clear();
        self.last_edit = None;
        self.suggested_for = self.query.clone();
        self.more = Load::Idle;
        let src = cx.core.source.clone();
        self.request = match self.filter {
            None => sp.load(async move { src.search(&q).await.map(Response::Grouped) }),
            Some(f) => sp.load(async move { src.search_filtered(&q, f).await.map(Response::Filtered) }),
        };
    }

    fn load_more(&mut self, sp: &Spawner, cx: &Ctx) {
        let Some(next) = self.filtered.as_ref().and_then(|l| l.next.clone()) else { return };
        if self.more.is_loading() {
            return;
        }
        let src = cx.core.source.clone();
        self.more = sp.load(async move { src.search_more(next).await });
    }

    fn poll(&mut self) {
        if let Some(done) = self.request.take_done() {
            match done {
                Ok(Response::Grouped(r)) => {
                    self.grouped = Some(r);
                    self.filtered = None;
                    self.error = None;
                }
                Ok(Response::Filtered(page)) => {
                    let mut list = FilteredList::new();
                    list.extend(page);
                    self.filtered = Some(list);
                    self.grouped = None;
                    self.error = None;
                }
                Err(e) => self.error = Some((e, false)),
            }
        }
        if let Some(done) = self.more.take_done() {
            match done {
                Ok(page) => {
                    if let Some(list) = &mut self.filtered {
                        list.extend(page);
                    }
                }
                Err(e) => self.error = Some((e, true)),
            }
        }
        if let Some(done) = self.suggest.take_done() {
            // Suggestions are best effort.
            self.suggestions = done.unwrap_or_default();
        }
    }

    pub fn ui(&mut self, ui: &mut Ui, sp: &Spawner, cx: &Ctx, out: &mut Vec<Action>) {
        self.poll();

        let mut submit = false;
        ui.horizontal(|ui| {
            let edit = egui::TextEdit::singleline(&mut self.query)
                .hint_text("Search songs, albums, artists, playlists")
                .desired_width(ui.available_width() - 90.0);
            let resp = ui.add(edit);
            if std::mem::take(&mut self.focus_query) {
                resp.request_focus();
            }
            if resp.changed() {
                self.last_edit = Some(Instant::now());
            }
            if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                submit = true;
            }
            if ui.button("🔍 Search").clicked() {
                submit = true;
            }
            self.suggestions_popup(&resp, &mut submit);
        });
        self.debounce_suggestions(ui, sp, cx);

        ui.horizontal(|ui| {
            let before = self.filter;
            ui.selectable_value(&mut self.filter, None, "All");
            for f in SearchFilter::ALL {
                ui.selectable_value(&mut self.filter, Some(f), f.label());
            }
            if self.filter != before && self.active.is_some() {
                submit = true;
            }
        });
        if submit {
            self.submit(sp, cx);
        }
        ui.separator();

        if self.request.is_loading() {
            widgets::loading(ui);
        }
        if let Some((err, more)) = &self.error
            && widgets::error_box(ui, "Search", err) {
                if *more {
                    self.error = None;
                    self.load_more(sp, cx);
                } else {
                    self.error = None;
                    if let Some((q, f)) = self.active.clone() {
                        self.query = q;
                        self.filter = f;
                        self.submit(sp, cx);
                    }
                }
            }

        let query = self.active.as_ref().map(|(q, _)| q.clone()).unwrap_or_default();
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            if let Some(r) = &self.grouped {
                if r.is_empty() {
                    no_results(ui, &query);
                } else {
                    grouped_ui(ui, cx, r, out);
                }
            } else if let Some(list) = &self.filtered {
                if list.items.is_empty() {
                    no_results(ui, &query);
                    return;
                }
                filtered_ui(ui, cx, &list.items, out);
                if list.next.is_some() {
                    // Reaching the end of the list loads the next page.
                    let sentinel = ui.horizontal(|ui| {
                        if self.more.is_loading() {
                            ui.spinner();
                        }
                        ui.label(RichText::new("Loading more…").weak());
                    });
                    let want_more = ui.is_rect_visible(sentinel.response.rect) && self.error.is_none();
                    if want_more {
                        self.load_more(sp, cx);
                    }
                }
            } else if self.active.is_none() && !self.request.is_loading() {
                ui.add_space(40.0);
                ui.vertical_centered(|ui| ui.label(RichText::new("Search YouTube Music").heading().weak()));
            }
        });
    }

    fn debounce_suggestions(&mut self, ui: &Ui, sp: &Spawner, cx: &Ctx) {
        let Some(t) = self.last_edit else { return };
        let q = self.query.trim().to_owned();
        if q.is_empty() {
            self.suggestions.clear();
            self.last_edit = None;
            return;
        }
        let elapsed = t.elapsed();
        if elapsed < SUGGEST_DEBOUNCE {
            ui.ctx().request_repaint_after(SUGGEST_DEBOUNCE - elapsed);
            return;
        }
        self.last_edit = None;
        if self.query != self.suggested_for {
            self.suggested_for = self.query.clone();
            let src = cx.core.source.clone();
            self.suggest = sp.load(async move { src.suggestions(&q).await });
        }
    }

    fn suggestions_popup(&mut self, resp: &egui::Response, submit: &mut bool) {
        let open = (resp.has_focus() || self.popup_hovered)
            && !self.suggestions.is_empty()
            && self.query == self.suggested_for;
        let mut chosen = None;
        let shown = egui::Popup::from_response(resp).open(open).width(resp.rect.width()).show(|ui| {
            for s in &self.suggestions {
                if ui.selectable_label(false, s).clicked() {
                    chosen = Some(s.clone());
                }
            }
        });
        // Pressing on a suggestion takes focus from the text box before the click completes.
        self.popup_hovered = shown.is_some_and(|r| r.response.contains_pointer());
        if let Some(s) = chosen {
            self.popup_hovered = false;
            self.query = s;
            *submit = true;
        }
    }
}

fn no_results(ui: &mut Ui, query: &str) {
    ui.add_space(20.0);
    ui.vertical_centered(|ui| ui.label(RichText::new(format!("No results for “{query}”")).weak()));
}

fn section(ui: &mut Ui, title: &str) {
    ui.add_space(8.0);
    ui.label(RichText::new(title).heading());
    ui.add_space(4.0);
}

fn grouped_ui(ui: &mut Ui, cx: &Ctx, r: &SearchResults, out: &mut Vec<Action>) {
    if !r.songs.is_empty() {
        section(ui, "Songs");
        songs_ui(ui, cx, &r.songs, out);
    }
    if !r.albums.is_empty() {
        section(ui, "Albums");
        for a in &r.albums {
            album_row(ui, a, out);
        }
    }
    if !r.artists.is_empty() {
        section(ui, "Artists");
        for a in &r.artists {
            artist_row(ui, a, out);
        }
    }
    if !r.playlists.is_empty() {
        section(ui, "Playlists");
        for p in &r.playlists {
            playlist_row(ui, p, out);
        }
    }
}

fn songs_ui(ui: &mut Ui, cx: &Ctx, songs: &[Track], out: &mut Vec<Action>) {
    for t in songs {
        // Playing a search result plays just that song.
        widgets::track_row(ui, cx, t, std::slice::from_ref(t), 0, RowOpts::default(), out);
    }
}

fn filtered_ui(ui: &mut Ui, cx: &Ctx, items: &[SearchItem], out: &mut Vec<Action>) {
    for item in items {
        match item {
            SearchItem::Song(t) => widgets::track_row(ui, cx, t, std::slice::from_ref(t), 0, RowOpts::default(), out),
            SearchItem::Album(a) => album_row(ui, a, out),
            SearchItem::Artist(a) => artist_row(ui, a, out),
            SearchItem::Playlist(p) => playlist_row(ui, p, out),
        }
    }
}

/// A clickable row with thumbnail, title and subtitle.
fn link_row(ui: &mut Ui, thumb: Option<&str>, title: &str, subtitle: &str, target: Target, out: &mut Vec<Action>) {
    let row = ui.horizontal(|ui| {
        widgets::thumb(ui, thumb, 48.0);
        ui.vertical(|ui| {
            ui.add(egui::Label::new(title).truncate().selectable(false));
            ui.add(egui::Label::new(RichText::new(subtitle).weak().small()).truncate().selectable(false));
        });
    });
    let resp = ui.interact(row.response.rect, ui.scope_id().with(("link", title, subtitle)), egui::Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(row.response.rect, 4.0, ui.visuals().widgets.hovered.weak_bg_fill.gamma_multiply(0.3));
    }
    if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
        out.push(Action::Open(target));
    }
}

fn album_row(ui: &mut Ui, a: &AlbumSummary, out: &mut Vec<Action>) {
    let artists: Vec<&str> = a.artists.iter().map(|x| x.name.as_str()).collect();
    let mut sub = format!("Album · {}", artists.join(", "));
    if let Some(y) = a.year {
        sub.push_str(&format!(" · {y}"));
    }
    link_row(ui, a.thumbnail.as_deref(), &a.title, &sub, Target::Album(a.id.clone()), out);
}

fn artist_row(ui: &mut Ui, a: &ArtistSummary, out: &mut Vec<Action>) {
    link_row(ui, a.thumbnail.as_deref(), &a.name, "Artist", Target::Artist(a.id.clone()), out);
}

fn playlist_row(ui: &mut Ui, p: &PlaylistSummary, out: &mut Vec<Action>) {
    let mut sub = "Playlist".to_owned();
    if let Some(o) = &p.owner {
        sub.push_str(&format!(" · {o}"));
    }
    if let Some(n) = p.track_count {
        sub.push_str(&format!(" · {n} tracks"));
    }
    link_row(ui, p.thumbnail.as_deref(), &p.title, &sub, Target::RemotePlaylist(p.id.clone()), out);
}

#[cfg(test)]
mod tests {
    use super::*;
    use koi_core::model::{AlbumRef, ArtistRef};

    fn song(id: &str) -> SearchItem {
        SearchItem::Song(Track {
            video_id: id.into(),
            title: id.into(),
            artists: vec![ArtistRef { id: None, name: "A".into() }],
            album: Some(AlbumRef { id: "x".into(), name: "X".into() }),
            duration_s: None,
            thumbnail: None,
        })
    }

    #[test]
    fn pages_append_without_duplicates() {
        let mut l = FilteredList::new();
        l.extend(Page::last(vec![song("a"), song("b")]));
        l.extend(Page::last(vec![song("b"), song("c")]));
        let ids: Vec<_> = l.items.iter().map(item_key).collect();
        assert_eq!(ids, ["s:a", "s:b", "s:c"]);
    }
}
