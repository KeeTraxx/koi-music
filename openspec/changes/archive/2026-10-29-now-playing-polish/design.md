# Design

## Context

See proposal.md for the motivation. Current layout (`views/player.rs`, egui 0.37):

- The bar is `Panel::bottom("now-playing").exact_size(84.0)` with `add_space(8.0)` on top, then one `ui.horizontal` holding three parts: track info (`allocate_ui(280×56)`), the transport + progress column, and the right-to-left volume/queue group.
- `ui.horizontal` starts each row at the default interact height and grows it as taller widgets arrive. Each widget is centered against the row height *at the moment it is placed*. Shuffle (selectable label) and ⏮ are placed before the 20 pt play button, so they center against a short row and end up near the top. ⏭ and repeat come after it and center against the taller row. The same effect pins the 56 px info block to the top of the bar.
- `widgets::thumb` uses `fit_to_exact_size` with `maintain_aspect_ratio(true)`. A 16:9 video thumbnail becomes about 56×31 px and sits at the top of the space reserved for a square.
- `KoiMusicApp::back: Vec<Route>` is filled in `open()` and only read by the "⬅ Back" button in the central panel.

## Goals / Non-Goals

**Goals:**
- Center the thumbnail, transport row and volume group vertically in the bar. Give all transport buttons one shared center line.
- A square, centered thumbnail slot in the bar that also works for 16:9 images.

**Non-Goals:**
- Restyling buttons (icons, sizes, colors) beyond what alignment needs.
- Keyboard or mouse "back" shortcuts as a replacement for the removed button.
- Changing thumbnails outside the now-playing bar (the queue panel and lists use `thumb` too; see Decisions).

## Decisions

**1. Fixed-height rows with `Align::Center` instead of `ui.horizontal`.**
Lay out the bar with `allocate_ui_with_layout(vec2(available_width, BAR_INNER_H), Layout::left_to_right(Align::Center), …)`. Do the same for the transport row, with a fixed height equal to the play button's height. Because the row height is known before any widget is placed, every widget centers on the same line, whatever order they come in. Drop the `add_space(8.0)` hack.
*Alternatives:* give every transport button the play button's font size (changes their look, and still fragile if a widget is added later); `ui.horizontal_centered` (centers against the full remaining height of the parent, which inside the vertical transport column would push the progress bar off).

**2. Center the image inside a square slot.**
In the bar, allocate a 56×56 slot and draw the thumbnail centered inside it (for example a child UI with a centered layout, or `Image::paint_at` on a rect centered in the slot). The placeholder already fills the full square. Keep the change local to the bar, or add an opt-in to `widgets::thumb`. Do not change the shared helper's default, because list rows use it with different expectations.
*Alternative:* crop to a square (`Image::uv`). Rejected for now; letterboxing keeps the whole picture.

**3. Queue toggle becomes an edge handle.**
A fixed-width (22 pt), non-resizable `Panel::right("queue-handle")` with no inner margin. Its whole area is one click target that flips `now.queue_open`. It is added *after* the queue panel, because egui nests later side panels inside earlier ones. That puts the handle on the queue's inner edge when the queue is open, and on the window edge when it is closed. The label "Queue" with an arrow is painted as one galley rotated a quarter turn clockwise around its center (`TextShape::with_angle_and_anchor`). Because the arrow rotates with the text, the source uses ▼/▲ to show ◀/▶. Hover uses the widget hover fill. While open, the handle gets a light selection tint.
*Alternatives:* a side-nav entry. Tried first, but it sat with the page links even though the queue is a panel and not a page. A button inside the queue panel. Rejected, because it cannot reopen a closed panel. A new `Target::Queue` route. Rejected for the same reason as the nav entry.

**4. Remove back navigation completely.**
Delete the `back` field, its use in `open()`, and the button. Keeping the history stack without any consumer would be dead code.

## Risks / Trade-offs

- [The fixed bar height is a magic number tied to `exact_size(84.0)`] → Derive the inner height from `ui.available_height()` inside the panel rather than hard-coding a second constant.
- [Users who relied on Back to return from an artist or album page to search results now lose them] → Search keeps its query and results in `SearchView`, which is not dropped on navigation, so clicking "Search" in the nav returns to the same results. Check this during implementation.
- [The handle sits next to the queue panel's resize grip, so a click near the boundary may start a resize instead] → The grip is only a few points wide on the panel side; the 22 pt handle stays the larger target.
- [The handle takes 22 pt of width from the central panel even when the queue is closed] → Accepted; it keeps the toggle discoverable.
