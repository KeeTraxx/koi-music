//! Play queue: ordering, repeat and shuffle. Pure logic, no audio.

use std::time::Duration;

use rand::Rng;
use rand::seq::SliceRandom;

use crate::model::Track;

/// Within this time "previous" goes to the preceding item instead of restarting.
pub const PREVIOUS_RESTART_THRESHOLD: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RepeatMode {
    #[default]
    Off,
    All,
    One,
}

impl RepeatMode {
    /// Off → All → One → Off.
    pub fn cycle(self) -> Self {
        match self {
            Self::Off => Self::All,
            Self::All => Self::One,
            Self::One => Self::Off,
        }
    }
}

/// Unique id of an entry in the queue (the same track may be queued twice).
pub type ItemId = u64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueItem {
    pub id: ItemId,
    pub track: Track,
}

/// What "previous" should do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Previous {
    /// Seek the current track to 0:00.
    Restart,
    /// Play the item at this index.
    Play(usize),
}

#[derive(Debug, Clone, Default)]
pub struct Queue {
    items: Vec<QueueItem>,
    current: Option<usize>,
    repeat: RepeatMode,
    shuffle: bool,
    /// Item order before shuffling, restored on unshuffle.
    original_order: Option<Vec<ItemId>>,
    next_id: ItemId,
}

impl Queue {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn items(&self) -> &[QueueItem] {
        &self.items
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn current_index(&self) -> Option<usize> {
        self.current
    }

    pub fn current(&self) -> Option<&QueueItem> {
        self.current.and_then(|i| self.items.get(i))
    }

    pub fn repeat(&self) -> RepeatMode {
        self.repeat
    }

    pub fn set_repeat(&mut self, mode: RepeatMode) {
        self.repeat = mode;
    }

    pub fn shuffle(&self) -> bool {
        self.shuffle
    }

    fn make_item(&mut self, track: Track) -> QueueItem {
        self.next_id += 1;
        QueueItem { id: self.next_id, track }
    }

    /// Replace the queue with `tracks` and make `start` current.
    /// If shuffle is on, the items after `start` are shuffled.
    pub fn set(&mut self, tracks: Vec<Track>, start: usize, rng: &mut impl Rng) {
        self.items = tracks.into_iter().map(|t| self.make_item(t)).collect();
        self.current = if self.items.is_empty() { None } else { Some(start.min(self.items.len() - 1)) };
        self.original_order = None;
        if self.shuffle {
            self.apply_shuffle(rng);
        }
    }

    /// Add to the end of the queue.
    pub fn append(&mut self, track: Track) -> ItemId {
        let item = self.make_item(track);
        let id = item.id;
        self.items.push(item);
        if let Some(order) = &mut self.original_order {
            order.push(id);
        }
        id
    }

    /// Insert right after the current item (or at the end if nothing is current).
    pub fn play_next(&mut self, track: Track) -> ItemId {
        let item = self.make_item(track);
        let id = item.id;
        let at = self.current.map_or(self.items.len(), |c| c + 1);
        let current_id = self.current().map(|c| c.id);
        if let Some(order) = &mut self.original_order {
            let pos = current_id
                .and_then(|cur| order.iter().position(|i| *i == cur))
                .map_or(order.len(), |p| p + 1);
            order.insert(pos, id);
        }
        self.items.insert(at, item);
        id
    }

    /// Remove the item at `index`. Returns true if it was the current item; the
    /// current index then points at the item that followed it (or is cleared if none).
    pub fn remove(&mut self, index: usize) -> bool {
        if index >= self.items.len() {
            return false;
        }
        let removed = self.items.remove(index);
        if let Some(order) = &mut self.original_order {
            order.retain(|i| *i != removed.id);
        }
        match self.current {
            Some(c) if c == index => {
                if index >= self.items.len() {
                    self.current = None;
                }
                true
            }
            Some(c) if c > index => {
                self.current = Some(c - 1);
                false
            }
            _ => false,
        }
    }

    /// Move the item at `from` so that it ends up at index `to`.
    pub fn move_item(&mut self, from: usize, to: usize) {
        if from >= self.items.len() || to >= self.items.len() || from == to {
            return;
        }
        let item = self.items.remove(from);
        self.items.insert(to, item);
        if let Some(c) = self.current {
            self.current = Some(if c == from {
                to
            } else if from < c && to >= c {
                c - 1
            } else if from > c && to <= c {
                c + 1
            } else {
                c
            });
        }
    }

    pub fn clear(&mut self) {
        self.items.clear();
        self.current = None;
        self.original_order = None;
    }

    /// Make `index` current.
    pub fn jump(&mut self, index: usize) -> Option<&QueueItem> {
        if index < self.items.len() {
            self.current = Some(index);
        }
        self.current()
    }

    /// The current track finished on its own. Returns the index to play next, if any.
    pub fn on_track_end(&mut self) -> Option<usize> {
        let cur = self.current?;
        let next = match self.repeat {
            RepeatMode::One => Some(cur),
            RepeatMode::All if cur + 1 >= self.items.len() => Some(0),
            _ if cur + 1 < self.items.len() => Some(cur + 1),
            _ => None,
        };
        if let Some(n) = next {
            self.current = Some(n);
        }
        next
    }

    /// User pressed "next". Repeat-one does not hold the user on the same track.
    pub fn advance(&mut self) -> Option<usize> {
        let cur = self.current?;
        let next = if cur + 1 < self.items.len() {
            Some(cur + 1)
        } else if self.repeat == RepeatMode::All {
            Some(0)
        } else {
            None
        };
        if let Some(n) = next {
            self.current = Some(n);
        }
        next
    }

    /// User pressed "previous" at playback `position` of the current track.
    pub fn previous(&mut self, position: Duration) -> Option<Previous> {
        let cur = self.current?;
        if position > PREVIOUS_RESTART_THRESHOLD {
            return Some(Previous::Restart);
        }
        let prev = if cur > 0 {
            cur - 1
        } else if self.repeat == RepeatMode::All {
            self.items.len() - 1
        } else {
            return Some(Previous::Restart);
        };
        self.current = Some(prev);
        Some(Previous::Play(prev))
    }

    /// Enable/disable shuffle. Enabling randomizes the items after the current one;
    /// disabling restores the order from before shuffling. The current item stays current.
    pub fn set_shuffle(&mut self, on: bool, rng: &mut impl Rng) {
        if on == self.shuffle {
            return;
        }
        self.shuffle = on;
        if on {
            self.apply_shuffle(rng);
        } else if let Some(order) = self.original_order.take() {
            let current_id = self.current().map(|i| i.id);
            let rank = |id: ItemId| order.iter().position(|o| *o == id).unwrap_or(usize::MAX);
            self.items.sort_by_key(|i| rank(i.id));
            self.current = current_id.and_then(|id| self.items.iter().position(|i| i.id == id));
        }
    }

    fn apply_shuffle(&mut self, rng: &mut impl Rng) {
        self.original_order = Some(self.items.iter().map(|i| i.id).collect());
        let start = self.current.map_or(0, |c| c + 1);
        if start < self.items.len() {
            self.items[start..].shuffle(rng);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ytm::fake::track;
    use rand::SeedableRng;
    use rand::rngs::StdRng;

    fn rng() -> StdRng {
        StdRng::seed_from_u64(7)
    }

    fn ids(q: &Queue) -> Vec<String> {
        q.items().iter().map(|i| i.track.video_id.clone()).collect()
    }

    fn current_id(q: &Queue) -> Option<String> {
        q.current().map(|i| i.track.video_id.clone())
    }

    fn queue(tracks: &[&str], start: usize) -> Queue {
        let mut q = Queue::new();
        q.set(tracks.iter().map(|t| track(t)).collect(), start, &mut rng());
        q
    }

    #[test]
    fn play_album_replaces_queue_and_starts_first() {
        let mut q = queue(&["x"], 0);
        q.set(vec![track("a"), track("b"), track("c")], 0, &mut rng());
        assert_eq!(ids(&q), ["a", "b", "c"]);
        assert_eq!(current_id(&q).as_deref(), Some("a"));
    }

    #[test]
    fn play_next_inserts_after_current() {
        let mut q = queue(&["a", "b", "c"], 0);
        q.play_next(track("n"));
        assert_eq!(ids(&q), ["a", "n", "b", "c"]);
        assert_eq!(q.on_track_end(), Some(1));
        assert_eq!(current_id(&q).as_deref(), Some("n"));
    }

    #[test]
    fn append_remove_reorder_clear() {
        let mut q = queue(&["a", "b", "c"], 1);
        q.append(track("d"));
        assert_eq!(ids(&q), ["a", "b", "c", "d"]);
        assert!(!q.remove(0));
        assert_eq!(current_id(&q).as_deref(), Some("b"));
        q.move_item(2, 0); // d to front
        assert_eq!(ids(&q), ["d", "b", "c"]);
        assert_eq!(current_id(&q).as_deref(), Some("b"));
        q.move_item(1, 2); // move current
        assert_eq!(ids(&q), ["d", "c", "b"]);
        assert_eq!(q.current_index(), Some(2));
        q.clear();
        assert!(q.is_empty());
        assert!(q.current().is_none());
    }

    #[test]
    fn removing_current_moves_to_following_item() {
        let mut q = queue(&["a", "b", "c"], 1);
        assert!(q.remove(1));
        assert_eq!(current_id(&q).as_deref(), Some("c"));
        assert!(q.remove(1));
        assert!(q.current().is_none());
    }

    #[test]
    fn duplicates_are_distinct_items() {
        let mut q = queue(&["a"], 0);
        q.append(track("a"));
        assert_ne!(q.items()[0].id, q.items()[1].id);
    }

    #[test]
    fn end_of_queue_with_repeat_off_stops_on_last() {
        let mut q = queue(&["a", "b"], 1);
        assert_eq!(q.on_track_end(), None);
        assert_eq!(current_id(&q).as_deref(), Some("b"));
    }

    #[test]
    fn auto_advance() {
        let mut q = queue(&["a", "b"], 0);
        assert_eq!(q.on_track_end(), Some(1));
    }

    #[test]
    fn repeat_one_replays_current() {
        let mut q = queue(&["a", "b"], 0);
        q.set_repeat(RepeatMode::One);
        assert_eq!(q.on_track_end(), Some(0));
        // Explicit next still moves on.
        assert_eq!(q.advance(), Some(1));
    }

    #[test]
    fn repeat_all_wraps() {
        let mut q = queue(&["a", "b"], 1);
        q.set_repeat(RepeatMode::All);
        assert_eq!(q.on_track_end(), Some(0));
        q.jump(1);
        assert_eq!(q.advance(), Some(0));
    }

    #[test]
    fn previous_rule() {
        let mut q = queue(&["a", "b", "c"], 1);
        assert_eq!(q.previous(Duration::from_secs(10)), Some(Previous::Restart));
        assert_eq!(q.current_index(), Some(1));
        assert_eq!(q.previous(Duration::from_secs(2)), Some(Previous::Play(0)));
        assert_eq!(q.current_index(), Some(0));
        // First item without repeat: restart.
        assert_eq!(q.previous(Duration::from_secs(1)), Some(Previous::Restart));
    }

    #[test]
    fn shuffle_keeps_current_and_unshuffle_restores_order() {
        let tracks: Vec<String> = (0..20).map(|i| i.to_string()).collect();
        let refs: Vec<&str> = tracks.iter().map(String::as_str).collect();
        let mut q = queue(&refs, 5);
        let before = ids(&q);
        q.set_shuffle(true, &mut rng());
        assert!(q.shuffle());
        assert_eq!(current_id(&q).as_deref(), Some("5"));
        assert_eq!(&ids(&q)[..6], &before[..6], "items before and at current are unchanged");
        assert_ne!(ids(&q), before, "remaining items are shuffled");
        let mut sorted = ids(&q);
        sorted.sort();
        let mut expected = before.clone();
        expected.sort();
        assert_eq!(sorted, expected, "same items");

        q.advance();
        let now = current_id(&q);
        q.set_shuffle(false, &mut rng());
        assert_eq!(ids(&q), before);
        assert_eq!(current_id(&q), now, "current track unchanged");
    }

    #[test]
    fn items_added_while_shuffled_survive_unshuffle() {
        let mut q = queue(&["a", "b", "c"], 0);
        q.set_shuffle(true, &mut rng());
        q.play_next(track("n"));
        q.append(track("z"));
        q.set_shuffle(false, &mut rng());
        assert_eq!(ids(&q), ["a", "n", "b", "c", "z"]);
    }

    #[test]
    fn cycle_repeat() {
        assert_eq!(RepeatMode::Off.cycle(), RepeatMode::All);
        assert_eq!(RepeatMode::All.cycle(), RepeatMode::One);
        assert_eq!(RepeatMode::One.cycle(), RepeatMode::Off);
    }
}
