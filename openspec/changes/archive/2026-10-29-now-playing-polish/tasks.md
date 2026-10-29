# Tasks

## 1. Now-playing bar alignment

- [x] 1.1 In `views/player.rs`, replace the outer `ui.horizontal` and the `add_space(8.0)` with a left-to-right `Align::Center` layout that fills the panel's available height, so the info block, transport column and volume group share one vertical center. Verify: with a track playing, the info block and the volume slider are centered between the bar's top and bottom edges.
- [x] 1.2 Put the transport buttons (shuffle, ⏮, play/pause, ⏭, repeat) in a fixed-height `Align::Center` row as tall as the play button. Verify: in a screenshot, all five icons share one horizontal center line in the Playing, Paused and Loading states.
- [x] 1.3 Draw the bar thumbnail centered in a 56×56 slot, without changing how `widgets::thumb` behaves in lists and the queue panel. Verify: a square album cover and a 16:9 video thumbnail both sit centered next to the title/artist block, and the "Nothing playing" placeholder is unchanged.

## 2. Queue toggle as an edge handle

- [x] 2.1 Remove the "☰ Queue" toggle from the bar's right-hand group, keeping mute and the volume slider. Verify: the bar shows only the volume controls on the right.
- [x] 2.2 Add `player::queue_handle` in a narrow right panel added after the queue panel: vertical "Queue" text with an arrow, the whole strip clickable, a hover fill, and a tint while open. Verify: clicking the handle at the window edge opens the panel, the handle moves to the panel's inner edge with the arrow flipped, and clicking it again closes the panel. The side nav has no Queue entry.

## 3. Remove back navigation

- [x] 3.1 Delete the `back` field, its pushes in `open()`, and the "⬅ Back" button in the central panel of `app.rs`. Verify: `cargo build` passes with no unused-field warnings, and no page shows a Back button.
- [x] 3.2 Check that leaving search and returning through the nav "Search" entry keeps the previous query and results. Verify: search, open an album, click "Search" in the nav, and the earlier results are still listed.

## 4. Search header polish

- [x] 4.1 In `views/search.rs`, give the query box an inner margin and some space above and below it, and make the Search button as tall as the box. Verify: in a screenshot, the query text has visible padding and the button and box share their top and bottom edges.
- [x] 4.2 Add an icon in front of each filter tab (All, Songs, Albums, Artists, Playlists), mapped in the UI crate. Verify: all five icons render with the bundled fonts and switching tabs still reruns the search.

## 5. Integration checks

- [x] 5.1 Run `cargo clippy --workspace --all-targets --locked -- -D warnings` and `cargo test --workspace --locked`. Verify: both succeed.
- [x] 5.2 Launch the app at the default window size and at a narrow width, and capture a screenshot of the bar while playing. Verify: the image and the buttons are centered, the queue toggle is only the edge handle, and nothing in the bar is clipped or overlaps.
