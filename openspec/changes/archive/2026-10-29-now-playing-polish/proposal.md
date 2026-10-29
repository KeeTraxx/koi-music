# Proposal

## Why

The now-playing bar looks unfinished: the thumbnail hugs the top edge, the shuffle and previous buttons sit higher than next and repeat, and the "☰ Queue" text toggle next to the volume slider does not match anything around it. The "⬅ Back" button above every page adds clutter without much value, because the side navigation already reaches every page in one click.

## What Changes

- Center the thumbnail and track info vertically in the now-playing bar.
- Align all transport buttons (shuffle, previous, play/pause, next, repeat) on one vertical center line.
- Remove the "☰ Queue" toggle from the now-playing bar. Add a narrow handle on the right edge of the window with vertical "Queue" text. Clicking it shows or hides the queue panel. While the panel is open, the handle sits on the panel's inner edge and its arrow points the way the panel will close.
- Give the search box more padding and space around it, make the Search button as tall as the box, and add icons to the result filter tabs (All, Songs, Albums, Artists, Playlists).
- **BREAKING (UI)**: Remove the "⬅ Back" button and the navigation history behind it. Users navigate with the side navigation and with links on pages (for example artist names).

## Capabilities

### New Capabilities
<!-- none -->

### Modified Capabilities
<!-- none -->

No spec-level behavior changes. No existing spec covers the bar's layout, where the queue toggle sits, or back navigation, so the change sets `skip_specs: true`.

## Impact

- `crates/koi-music/src/views/player.rs`: now-playing bar layout; the queue toggle is removed from it; new queue handle widget.
- `crates/koi-music/src/views/search.rs`: search box padding and filter tab icons (icons live in the UI crate; `SearchFilter::label()` in core is unchanged).
- `crates/koi-music/src/app.rs`: the `back` route stack and the Back button are removed; a narrow right panel hosts the queue handle.
- No core (`koi-core`) changes, no new dependencies.
