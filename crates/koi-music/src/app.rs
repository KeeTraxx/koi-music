//! The egui application: navigation, panels, dialogs and the central action handler.

use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui::{self, RichText, Ui};
use koi_core::app::{AppCore, ToolProbe};
use koi_core::import::{RemoteCollection, import_collection};
use koi_core::library::{EntryId, Library, LibrarySnapshot, PlaylistId};
use koi_core::model::Track;
use koi_core::media::MediaSession;
use koi_core::player::{PlayerCmd, PlayerState};
use tokio::sync::mpsc;

use crate::fetch::Spawner;
use crate::views::{detail, downloads, library, player, search::SearchView, settings::SettingsView};

const TOAST_TIME: Duration = Duration::from_secs(5);

/// Something the user can navigate to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Search,
    Album(String),
    RemotePlaylist(String),
    Artist(String),
    LocalPlaylist(PlaylistId),
    Downloads,
    History,
    Settings,
}

/// User intents collected while drawing a frame and handled afterwards.
pub enum Action {
    Play { tracks: Vec<Track>, start: usize },
    PlayShuffled(Vec<Track>),
    PlayNext(Track),
    Enqueue(Vec<Track>),
    AddToPlaylist { playlist: PlaylistId, tracks: Vec<Track>, confirmed: bool },
    NewPlaylist { with: Vec<Track> },
    RenamePlaylist(PlaylistId),
    DeletePlaylist(PlaylistId),
    RemoveFromPlaylist { playlist: PlaylistId, entry: EntryId },
    MovePlaylistEntry { playlist: PlaylistId, from: usize, to: usize },
    SaveAsPlaylist(RemoteCollection),
    Download(Vec<Track>),
    DeleteDownload(String),
    Open(Target),
    Player(PlayerCmd),
    Toast(String),
}

/// Read-only state shared by all views for one frame.
pub struct Ctx<'a> {
    pub core: &'a AppCore,
    pub lib: Arc<LibrarySnapshot>,
    pub player: Arc<PlayerState>,
    /// yt-dlp is available, so non-downloaded tracks can be streamed and downloaded.
    pub can_stream: bool,
}

enum Route {
    Search,
    Detail(Target, Box<detail::Page>),
    LocalPlaylist(PlaylistId),
    Downloads,
    History,
    Settings,
}

impl Route {
    fn target(&self) -> Target {
        match self {
            Self::Search => Target::Search,
            Self::Detail(t, _) => t.clone(),
            Self::LocalPlaylist(id) => Target::LocalPlaylist(*id),
            Self::Downloads => Target::Downloads,
            Self::History => Target::History,
            Self::Settings => Target::Settings,
        }
    }
}

enum Dialog {
    NewPlaylist { name: String, with: Vec<Track>, error: Option<String> },
    Rename { id: PlaylistId, name: String, error: Option<String> },
    ConfirmDelete { id: PlaylistId, name: String },
    ConfirmDuplicate { playlist: PlaylistId, name: String, tracks: Vec<Track>, dupes: usize },
}

pub struct KoiMusicApp {
    core: AppCore,
    sp: Spawner,
    route: Route,
    back: Vec<Route>,
    search: SearchView,
    settings: SettingsView,
    now: player::NowPlaying,
    dialog: Option<Dialog>,
    toasts: Vec<(String, Instant)>,
    toast_tx: mpsc::UnboundedSender<String>,
    toast_rx: mpsc::UnboundedReceiver<String>,
    last_error_seq: u64,
    /// Last downloads notice shown (e.g. "downloads stopped" after a bot check).
    last_download_notice_seq: u64,
    /// OS media controls; `None` when the platform service is unavailable.
    media: Option<MediaSession>,
}

impl KoiMusicApp {
    pub fn new(core: AppCore, ctx: egui::Context, media: Option<MediaSession>) -> Self {
        let (toast_tx, toast_rx) = mpsc::unbounded_channel();
        let sp = Spawner { rt: core.rt.clone(), ctx };
        Self {
            core,
            sp,
            route: Route::Search,
            back: Vec::new(),
            search: SearchView::new(),
            settings: SettingsView::default(),
            now: player::NowPlaying::default(),
            dialog: None,
            toasts: Vec::new(),
            toast_tx,
            toast_rx,
            last_error_seq: 0,
            last_download_notice_seq: 0,
            media,
        }
    }

    fn toast(&mut self, msg: impl Into<String>) {
        self.toasts.push((msg.into(), Instant::now()));
    }

    /// Run a library mutation on a blocking thread; errors become toasts.
    fn db(&self, f: impl FnOnce(&Library) -> koi_core::Result<()> + Send + 'static) {
        let (lib, tx, ctx) = (self.core.library.clone(), self.toast_tx.clone(), self.sp.ctx.clone());
        self.core.rt.spawn_blocking(move || {
            if let Err(e) = f(&lib) {
                let _ = tx.send(e.to_string());
                ctx.request_repaint();
            }
        });
    }

    fn open(&mut self, target: Target, cx_lib: &Arc<LibrarySnapshot>, player: &Arc<PlayerState>) {
        if self.route.target() == target {
            return;
        }
        let route = match &target {
            Target::Search => Route::Search,
            Target::LocalPlaylist(id) => Route::LocalPlaylist(*id),
            Target::Downloads => Route::Downloads,
            Target::History => Route::History,
            Target::Settings => Route::Settings,
            Target::Album(_) | Target::RemotePlaylist(_) | Target::Artist(_) => {
                let cx = Ctx { core: &self.core, lib: cx_lib.clone(), player: player.clone(), can_stream: true };
                let Some(page) = detail::Page::open(&target, &self.sp, &cx) else { return };
                Route::Detail(target, Box::new(page))
            }
        };
        let old = std::mem::replace(&mut self.route, route);
        self.back.push(old);
        if self.back.len() > 50 {
            self.back.remove(0);
        }
    }

    fn handle(&mut self, action: Action, lib: &Arc<LibrarySnapshot>, player: &Arc<PlayerState>) {
        let p = &self.core.player;
        match action {
            Action::Play { tracks, start } => p.send(PlayerCmd::PlayTracks { tracks, start }),
            Action::PlayShuffled(tracks) => p.send(PlayerCmd::PlayShuffled(tracks)),
            Action::PlayNext(t) => {
                let msg = format!("Playing next: {}", t.title);
                p.send(PlayerCmd::PlayNext(t));
                self.toast(msg);
            }
            Action::Enqueue(tracks) => {
                let n = tracks.len();
                p.send(PlayerCmd::Append(tracks));
                self.toast(if n == 1 { "Added to queue".to_owned() } else { format!("Added {n} tracks to queue") });
            }
            Action::AddToPlaylist { playlist, tracks, confirmed } => {
                let Some(pl) = lib.playlist(playlist) else { return };
                let dupes = tracks.iter().filter(|t| pl.entries.iter().any(|e| e.track.video_id == t.video_id)).count();
                if dupes > 0 && !confirmed {
                    self.dialog = Some(Dialog::ConfirmDuplicate { playlist, name: pl.name.clone(), tracks, dupes });
                    return;
                }
                let msg = format!("Added to “{}”", pl.name);
                self.db(move |l| l.add_tracks(playlist, &tracks));
                self.toast(msg);
            }
            Action::NewPlaylist { with } => {
                self.dialog = Some(Dialog::NewPlaylist { name: String::new(), with, error: None });
            }
            Action::RenamePlaylist(id) => {
                if let Some(pl) = lib.playlist(id) {
                    self.dialog = Some(Dialog::Rename { id, name: pl.name.clone(), error: None });
                }
            }
            Action::DeletePlaylist(id) => {
                if let Some(pl) = lib.playlist(id) {
                    self.dialog = Some(Dialog::ConfirmDelete { id, name: pl.name.clone() });
                }
            }
            Action::RemoveFromPlaylist { playlist, entry } => self.db(move |l| l.remove_entry(playlist, entry)),
            Action::MovePlaylistEntry { playlist, from, to } => self.db(move |l| l.move_entry(playlist, from, to)),
            Action::SaveAsPlaylist(what) => {
                let (src, lib, tx, ctx) =
                    (self.core.source.clone(), self.core.library.clone(), self.toast_tx.clone(), self.sp.ctx.clone());
                self.core.rt.spawn(async move {
                    let msg = match import_collection(src.as_ref(), lib.clone(), &what).await {
                        Ok(id) => {
                            let name = lib.snapshot().playlist(id).map(|p| p.name.clone()).unwrap_or_default();
                            format!("Saved as playlist “{name}”")
                        }
                        Err(e) => format!("Could not save playlist: {e}"),
                    };
                    let _ = tx.send(msg);
                    ctx.request_repaint();
                });
                self.toast("Saving playlist…");
            }
            Action::Download(tracks) => {
                let n = self.core.downloads.download_all(tracks);
                self.toast(match n {
                    0 => "Already downloaded or downloading".to_owned(),
                    1 => "Download queued".to_owned(),
                    n => format!("{n} downloads queued"),
                });
            }
            Action::DeleteDownload(id) => {
                let (dl, tx) = (self.core.downloads.clone(), self.toast_tx.clone());
                self.sp.fire(async move { dl.delete(&id).await }, move |e| {
                    let _ = tx.send(format!("Could not delete: {e}"));
                });
            }
            Action::Open(target) => self.open(target, lib, player),
            Action::Player(cmd) => p.send(cmd),
            Action::Toast(m) => self.toast(m),
        }
    }

}

fn banners(ui: &mut Ui, core: &AppCore) {
    {
        match core.tool() {
            ToolProbe::Missing(e) => {
                ui.colored_label(
                    ui.visuals().error_fg_color,
                    format!(
                        "⚠ yt-dlp is not available ({e}). Streaming and downloads are disabled; downloaded tracks still play. \
                         Install yt-dlp and Deno (see README) or set the path in Settings."
                    ),
                );
                ui.separator();
            }
            ToolProbe::Ready(s) if s.js_runtimes.is_empty() => {
                ui.colored_label(
                    ui.visuals().warn_fg_color,
                    "⚠ yt-dlp found no JavaScript runtime. Install Deno for reliable playback and downloads.",
                );
                ui.separator();
            }
            _ => {}
        }
    }
}

impl KoiMusicApp {
    fn dialogs(&mut self, ctx: &egui::Context, out: &mut Vec<Action>) {
        let Some(dialog) = &mut self.dialog else { return };
        let mut close = false;
        let lib = self.core.library.clone();
        egui::Modal::new(egui::Id::unique("dialog")).show(ctx, |ui| {
            ui.set_width(340.0);
            match dialog {
                Dialog::NewPlaylist { name, with, error } => {
                    ui.heading("New playlist");
                    let resp = ui.add(egui::TextEdit::singleline(name).hint_text("Name"));
                    resp.request_focus();
                    if let Some(e) = error {
                        ui.colored_label(ui.visuals().error_fg_color, e.as_str());
                    }
                    let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    ui.horizontal(|ui| {
                        if ui.button("Create").clicked() || enter {
                            match lib.create_playlist(name) {
                                Ok(id) => {
                                    if !with.is_empty() {
                                        out.push(Action::AddToPlaylist { playlist: id, tracks: std::mem::take(with), confirmed: true });
                                    } else {
                                        out.push(Action::Open(Target::LocalPlaylist(id)));
                                    }
                                    close = true;
                                }
                                Err(e) => *error = Some(validation_message(&e)),
                            }
                        }
                        if ui.button("Cancel").clicked() {
                            close = true;
                        }
                    });
                }
                Dialog::Rename { id, name, error } => {
                    ui.heading("Rename playlist");
                    let resp = ui.add(egui::TextEdit::singleline(name));
                    if let Some(e) = error {
                        ui.colored_label(ui.visuals().error_fg_color, e.as_str());
                    }
                    let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    ui.horizontal(|ui| {
                        if ui.button("Rename").clicked() || enter {
                            match lib.rename_playlist(*id, name) {
                                Ok(()) => close = true,
                                Err(e) => *error = Some(validation_message(&e)),
                            }
                        }
                        if ui.button("Cancel").clicked() {
                            close = true;
                        }
                    });
                }
                Dialog::ConfirmDelete { id, name } => {
                    ui.heading("Delete playlist?");
                    ui.label(format!("“{name}” will be deleted. Downloaded files are kept."));
                    ui.horizontal(|ui| {
                        if ui.button("Delete").clicked() {
                            if let Err(e) = lib.delete_playlist(*id) {
                                out.push(Action::Toast(e.to_string()));
                            }
                            out.push(Action::Open(Target::Search));
                            close = true;
                        }
                        if ui.button("Cancel").clicked() {
                            close = true;
                        }
                    });
                }
                Dialog::ConfirmDuplicate { playlist, name, tracks, dupes } => {
                    ui.heading("Already in playlist");
                    let what = if tracks.len() == 1 {
                        format!("“{}” is already in “{name}”.", tracks[0].title)
                    } else {
                        format!("{dupes} of these tracks are already in “{name}”.")
                    };
                    ui.label(format!("{what} Add anyway?"));
                    ui.horizontal(|ui| {
                        if ui.button("Add anyway").clicked() {
                            out.push(Action::AddToPlaylist { playlist: *playlist, tracks: std::mem::take(tracks), confirmed: true });
                            close = true;
                        }
                        if ui.button("Cancel").clicked() {
                            close = true;
                        }
                    });
                }
            }
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                close = true;
            }
        });
        if close {
            self.dialog = None;
        }
    }

    fn toasts_ui(&mut self, ctx: &egui::Context) {
        while let Ok(m) = self.toast_rx.try_recv() {
            self.toast(m);
        }
        self.toasts.retain(|(_, t)| t.elapsed() < TOAST_TIME);
        if self.toasts.is_empty() {
            return;
        }
        egui::Area::new(egui::Id::unique("toasts"))
            .anchor(egui::Align2::RIGHT_BOTTOM, [-16.0, -110.0])
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                for (msg, _) in &self.toasts {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        ui.set_max_width(360.0);
                        ui.label(msg);
                    });
                }
            });
        ctx.request_repaint_after(Duration::from_millis(500));
    }
}

fn validation_message(e: &koi_core::Error) -> String {
    if e.kind == koi_core::ErrorKind::Invalid { "Please enter a name.".to_owned() } else { e.to_string() }
}

impl eframe::App for KoiMusicApp {
    /// Runs on every repaint, also while the window is hidden; the player requests a
    /// repaint on each state change, so the OS media controls stay current.
    fn logic(&mut self, _ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Some(media) = &mut self.media {
            media.update(&self.core.player.state());
        }
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let lib = self.core.library.snapshot();
        let player = self.core.player.state();
        if let Some(e) = &player.last_error
            && e.seq != self.last_error_seq {
                self.last_error_seq = e.seq;
                self.toast(e.message.clone());
            }
        if let Some(n) = &self.core.downloads.state().notice
            && n.seq != self.last_download_notice_seq
        {
            self.last_download_notice_seq = n.seq;
            self.toast(n.message.clone());
        }
        let mut out = Vec::new();
        {
            let Self { core, sp, route, back, search, settings, now, .. } = self;
            let cx = Ctx { core, lib: lib.clone(), player: player.clone(), can_stream: core.tool().is_usable() };

            egui::Panel::bottom("now-playing").exact_size(84.0).show(ui, |ui| {
                ui.add_space(8.0);
                now.ui(ui, &cx, &mut out);
            });
            egui::Panel::left("nav").default_size(220.0).resizable(true).show(ui, |ui| {
                nav(ui, route, &cx, &mut out);
            });
            if now.queue_open {
                egui::Panel::right("queue").default_size(320.0).min_size(240.0).max_size(640.0).resizable(true).show(ui, |ui| {
                    player::queue_ui(ui, &cx, &mut out);
                });
            }
            egui::CentralPanel::default_margins().show(ui, |ui| {
                banners(ui, cx.core);
                if !back.is_empty() && ui.button("⬅ Back").clicked()
                    && let Some(prev) = back.pop() {
                        *route = prev;
                    }
                match route {
                    Route::Search => search.ui(ui, sp, &cx, &mut out),
                    Route::Detail(_, page) => page.ui(ui, sp, &cx, &mut out),
                    Route::LocalPlaylist(id) => library::playlist_ui(ui, &cx, *id, &mut out),
                    Route::Downloads => downloads::downloads_ui(ui, &cx, &mut out),
                    Route::History => library::history_ui(ui, &cx, &mut out),
                    Route::Settings => settings.ui(ui, &cx, &mut out),
                }
            });
        }
        self.dialogs(&ctx, &mut out);
        for action in out {
            self.handle(action, &lib, &player);
        }
        self.toasts_ui(&ctx);
    }
}

/// Side navigation (free function so it can borrow the route while views borrow other fields).
fn nav(ui: &mut Ui, route: &Route, cx: &Ctx, out: &mut Vec<Action>) {
    let current = route.target();
    ui.add_space(8.0);
    ui.label(RichText::new("koi-music").heading().strong());
    ui.add_space(8.0);
    let mut item = |ui: &mut Ui, label: String, target: Target| {
        if ui.selectable_label(current == target, label).clicked() {
            out.push(Action::Open(target));
        }
    };
    item(ui, "🔍 Search".into(), Target::Search);
    item(ui, "🕘 Recently played".into(), Target::History);
    let dl = match downloads::nav_badge(cx) {
        Some(b) => format!("⬇ Downloads ({b})"),
        None => "⬇ Downloads".into(),
    };
    item(ui, dl, Target::Downloads);
    item(ui, "⚙ Settings".into(), Target::Settings);
    ui.separator();
    ui.horizontal(|ui| {
        ui.label(RichText::new("Playlists").strong());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button("➕").on_hover_text("New playlist").clicked() {
                out.push(Action::NewPlaylist { with: Vec::new() });
            }
        });
    });
    egui::ScrollArea::vertical().show(ui, |ui| {
        if cx.lib.playlists.is_empty() {
            ui.label(RichText::new("No playlists yet").weak().small());
        }
        for p in &cx.lib.playlists {
            let t = Target::LocalPlaylist(p.id);
            if ui.selectable_label(current == t, format!("☰ {}", p.name)).clicked() {
                out.push(Action::Open(t));
            }
        }
    });
}
