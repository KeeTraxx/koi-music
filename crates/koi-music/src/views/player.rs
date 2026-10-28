//! Now-playing bar and the queue panel.

use std::time::Duration;

use eframe::egui::{self, Id, RichText, Ui};
use egui_dnd::DragDropItem;
use koi_core::player::{PlayerCmd, PlayerState, Status};
use koi_core::queue::{QueueItem, RepeatMode};

use crate::app::{Action, Ctx, Target};
use crate::views::library::dnd_target;
use crate::widgets::{self, fmt_duration};

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
        ui.horizontal(|ui| {
            // Track info.
            ui.allocate_ui(egui::vec2(280.0, 56.0), |ui| {
                ui.horizontal(|ui| {
                    let t = s.current_track();
                    widgets::thumb(ui, t.and_then(|t| t.thumbnail.as_deref()), 56.0);
                    ui.vertical(|ui| {
                        ui.set_width(210.0);
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
            });

            // Transport + progress.
            ui.vertical(|ui| {
                ui.set_width((ui.available_width() - 260.0).max(200.0));
                ui.horizontal(|ui| {
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

            // Volume + queue toggle.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.selectable_label(self.queue_open, "☰ Queue").clicked() {
                    self.queue_open = !self.queue_open;
                }
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
