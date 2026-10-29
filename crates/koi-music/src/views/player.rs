//! Now-playing bar and the queue panel.

use std::time::Duration;

use eframe::egui::{self, Align, Id, Layout, RichText, Ui};
use egui_dnd::DragDropItem;
use koi_core::player::{PlayerCmd, PlayerState, Status};
use koi_core::queue::{QueueItem, RepeatMode};

use crate::app::{Action, Ctx, Target};
use crate::views::library::dnd_target;
use crate::widgets::{self, fmt_duration};

/// Size of the square cover slot in the now-playing bar.
const THUMB: f32 = 56.0;
/// Height of the transport row; at least as tall as the large play button so that
/// all buttons center on the same line.
const TRANSPORT_ROW_HEIGHT: f32 = 32.0;

/// Lay out `add` top-down, vertically centered in the parent's `Align::Center` row.
/// A column's height is only known after layout, so the height measured in the previous
/// frame is used (stored under `id`); a change triggers one extra repaint.
fn centered_column<R>(ui: &mut Ui, id: &str, width: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
    let id = ui.scope_id().with(id);
    let height = ui.ctx().data(|d| d.get_temp::<f32>(id)).unwrap_or(0.0);
    let r = ui.allocate_ui_with_layout(egui::vec2(width, height), Layout::top_down(Align::Min), |ui| {
        ui.set_width(width);
        add(ui)
    });
    let measured = r.response.rect.height();
    if (measured - height).abs() > 0.5 {
        ui.ctx().data_mut(|d| d.insert_temp(id, measured));
        ui.ctx().request_repaint();
    }
    r.inner
}

#[derive(Default)]
pub struct NowPlaying {
    /// Seek position while the user drags the progress slider.
    seek_drag: Option<f32>,
    /// Volume before muting via the button, for display only.
    pub queue_open: bool,
}

impl NowPlaying {
    pub fn ui(&mut self, ui: &mut Ui, cx: &Ctx, out: &mut Vec<Action>) {
        let s = &cx.player;
        let send = |out: &mut Vec<Action>, cmd| out.push(Action::Player(cmd));
        // A fixed-height row: `ui.horizontal` grows while widgets are added, which centers
        // early widgets against a shorter row than later ones.
        let bar_height = ui.available_height();
        ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), bar_height), Layout::left_to_right(Align::Center), |ui| {
            // Track info.
            ui.allocate_ui_with_layout(egui::vec2(280.0, bar_height), Layout::left_to_right(Align::Center), |ui| {
                ui.set_min_width(280.0);
                let t = s.current_track();
                // Square slot with the image centered in it, so 16:9 video thumbnails don't sit at the top.
                let (slot, _) = ui.allocate_exact_size(egui::Vec2::splat(THUMB), egui::Sense::hover());
                ui.scope_builder(egui::UiBuilder::new().max_rect(slot).layout(Layout::centered_and_justified(egui::Direction::TopDown)), |ui| {
                    widgets::thumb(ui, t.and_then(|t| t.thumbnail.as_deref()), THUMB);
                });
                centered_column(ui, "now-playing-info", 210.0, |ui| {
                    match t {
                        Some(t) => {
                            ui.add(egui::Label::new(RichText::new(&t.title).strong()).truncate());
                            let artist = ui.add(egui::Label::new(RichText::new(t.artist_line()).weak()).truncate().sense(egui::Sense::click()));
                            if let Some(id) = t.artists.iter().find_map(|a| a.id.clone())
                                && artist.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                                    out.push(Action::Open(Target::Artist(id)));
                                }
                            if s.local {
                                ui.label(RichText::new("⬇ local file").small().weak());
                            }
                        }
                        None => {
                            ui.label(RichText::new("Nothing playing").weak());
                        }
                    }
                });
            });

            // Transport + progress.
            let width = (ui.available_width() - 260.0).max(200.0);
            centered_column(ui, "now-playing-transport", width, |ui| {
                let row = egui::vec2(ui.available_width(), TRANSPORT_ROW_HEIGHT);
                ui.allocate_ui_with_layout(row, Layout::left_to_right(Align::Center), |ui| {
                    let shuffle = ui.selectable_label(s.shuffle, "🔀").on_hover_text("Shuffle");
                    if shuffle.clicked() {
                        send(out, PlayerCmd::SetShuffle(!s.shuffle));
                    }
                    if ui.button("⏮").on_hover_text("Previous").clicked() {
                        send(out, PlayerCmd::Previous);
                    }
                    let (icon, hint) = match s.status {
                        Status::Playing => ("⏸", "Pause"),
                        Status::Loading => ("⏳", "Loading"),
                        _ => ("⏵", "Play"),
                    };
                    let play = ui.add_enabled(s.current.is_some(), egui::Button::new(RichText::new(icon).size(20.0)));
                    if play.on_hover_text(hint).clicked() {
                        send(out, PlayerCmd::TogglePlay);
                    }
                    if ui.button("⏭").on_hover_text("Next").clicked() {
                        send(out, PlayerCmd::Next);
                    }
                    let (icon, hint) = match s.repeat {
                        RepeatMode::Off => ("🔁", "Repeat: off"),
                        RepeatMode::All => ("🔁", "Repeat: all"),
                        RepeatMode::One => ("🔂", "Repeat: one"),
                    };
                    if ui.selectable_label(s.repeat != RepeatMode::Off, icon).on_hover_text(hint).clicked() {
                        send(out, PlayerCmd::SetRepeat(s.repeat.cycle()));
                    }
                });
                self.progress(ui, s, out);
            });

            // Volume.
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let mut vol = s.volume;
                let slider = ui.add(egui::Slider::new(&mut vol, 0.0..=1.0).show_value(false));
                if slider.changed() {
                    send(out, PlayerCmd::SetVolume(vol));
                }
                slider.on_hover_text(format!("Volume {:.0}%", s.volume * 100.0));
                let icon = if s.muted || s.volume == 0.0 { "🔇" } else { "🔊" };
                if ui.button(icon).on_hover_text(if s.muted { "Unmute" } else { "Mute" }).clicked() {
                    send(out, PlayerCmd::SetMuted(!s.muted));
                }
            });
        });
    }

    fn progress(&mut self, ui: &mut Ui, s: &PlayerState, out: &mut Vec<Action>) {
        let total = s.duration.unwrap_or_default().as_secs_f32();
        let shown = self.seek_drag.unwrap_or(s.position.as_secs_f32());
        ui.horizontal(|ui| {
            ui.label(RichText::new(fmt_duration(Duration::from_secs_f32(shown))).monospace().small());
            ui.spacing_mut().slider_width = (ui.available_width() - 60.0).max(80.0);
            let mut v = shown;
            let enabled = total > 0.0 && matches!(s.status, Status::Playing | Status::Paused);
            let resp = ui.add_enabled(enabled, egui::Slider::new(&mut v, 0.0..=total.max(1.0)).show_value(false));
            if resp.dragged() {
                self.seek_drag = Some(v);
            }
            if resp.drag_stopped() || (resp.changed() && !resp.dragged()) {
                self.seek_drag = None;
                out.push(Action::Player(PlayerCmd::Seek(Duration::from_secs_f32(v))));
            }
            ui.label(RichText::new(s.duration.map(fmt_duration).unwrap_or_else(|| "--:--".into())).monospace().small());
        });
        if s.status == Status::Playing {
            ui.ctx().request_repaint_after(Duration::from_millis(250));
        }
    }
}

/// Width of the queue handle strip on the right edge of the window.
pub const QUEUE_HANDLE_WIDTH: f32 = 22.0;

/// A clickable strip with vertical "Queue" text that shows or hides the queue panel.
/// It fills its whole (narrow) panel, so it can be hit anywhere along the window edge.
pub fn queue_handle(ui: &mut Ui, open: &mut bool) {
    let rect = ui.max_rect();
    let resp = ui.allocate_rect(rect, egui::Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
    let resp = resp.on_hover_text(if *open { "Hide queue" } else { "Show queue" });
    if resp.clicked() {
        *open = !*open;
    }
    let visuals = ui.visuals();
    let (fill, color) = if resp.hovered() {
        (visuals.widgets.hovered.weak_bg_fill, visuals.widgets.hovered.fg_stroke.color)
    } else if *open {
        (visuals.selection.bg_fill.gamma_multiply(0.2), visuals.strong_text_color())
    } else {
        (egui::Color32::TRANSPARENT, visuals.text_color())
    };
    let painter = ui.painter();
    painter.rect_filled(rect, 0.0, fill);
    // Arrow points the way the panel will move. The text is rotated a quarter turn clockwise
    // (reads top to bottom), so "▲" ends up pointing right and "▼" left.
    let label = if *open { "▲  Queue" } else { "▼  Queue" };
    let galley = painter.layout_no_wrap(label.to_owned(), egui::FontId::proportional(14.0), color);
    let pos = rect.center() - galley.size() / 2.0;
    let text = egui::epaint::TextShape::new(pos, galley, color)
        .with_angle_and_anchor(std::f32::consts::FRAC_PI_2, egui::Align2::CENTER_CENTER);
    painter.add(text);
}

struct DndQueueItem<'a> {
    item: &'a QueueItem,
    index: usize,
}

impl DragDropItem for DndQueueItem<'_> {
    fn id(&self) -> Id {
        Id::unique(("queue-item", self.item.id))
    }
}

pub fn queue_ui(ui: &mut Ui, cx: &Ctx, out: &mut Vec<Action>) {
    let s = &cx.player;
    ui.horizontal(|ui| {
        ui.label(RichText::new("Queue").heading());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.add_enabled(!s.queue.is_empty(), egui::Button::new("Clear")).clicked() {
                out.push(Action::Player(PlayerCmd::Clear));
            }
        });
    });
    ui.separator();
    if s.queue.is_empty() {
        ui.label(RichText::new("The queue is empty.").weak());
        return;
    }
    egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        let items = s.queue.iter().enumerate().map(|(index, item)| DndQueueItem { item, index });
        let response = egui_dnd::dnd(ui, Id::unique("queue-dnd")).show(items, |ui, it, handle, _state| {
            let current = s.current == Some(it.index);
            ui.horizontal(|ui| {
                widgets::drag_handle(ui, handle);
                widgets::thumb(ui, it.item.track.thumbnail.as_deref(), 32.0);
                let mut title = RichText::new(&it.item.track.title);
                if current {
                    title = title.strong().color(ui.visuals().selection.stroke.color);
                }
                // Button first (right-aligned); the text truncates to the remaining width,
                // so a row is never wider than the panel.
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("✖").on_hover_text("Remove").clicked() {
                        out.push(Action::Player(PlayerCmd::Remove(it.index)));
                    }
                    let label = ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                        ui.add(egui::Label::new(title).truncate().selectable(false));
                        let artist = RichText::new(it.item.track.artist_line()).weak().small();
                        ui.add(egui::Label::new(artist).truncate().selectable(false));
                    });
                    let resp = ui.interact(label.response.rect, ui.scope_id().with(("q", it.item.id)), egui::Sense::click());
                    if resp.clicked() {
                        out.push(Action::Player(PlayerCmd::Jump(it.index)));
                    }
                });
            });
        });
        if let Some(update) = response.final_update() {
            let to = dnd_target(update.from, update.to);
            if to != update.from {
                out.push(Action::Player(PlayerCmd::Move { from: update.from, to }));
            }
        }
    });
}
