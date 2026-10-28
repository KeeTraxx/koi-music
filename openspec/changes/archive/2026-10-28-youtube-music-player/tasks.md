# Tasks

## 1. Project scaffolding

- [x] 1.1 Create Cargo workspace (license GPL-3.0-or-later) with `crates/rsm-core` (lib) and `crates/rs-music` (eframe bin); verify `cargo build --workspace` succeeds
- [x] 1.2 Set up eframe app shell with app id `ch.compile.rs-music`, tokio runtime bridge (design D3), side panel navigation, and empty views; verify `cargo run -p rs-music` opens a window with navigation working
- [x] 1.3 Add `ProjectDirs` app directories (design D5) and a README section listing build deps (ALSA dev libs) and runtime deps (yt-dlp, a JS runtime such as Deno, optional ffmpeg, a D-Bus session for MPRIS; all user-installed, never bundled; optional yt-dlp PO-token provider plugin against bot checks) plus a Nix dev shell; verify a clean shell builds the app
- [x] 1.4 Add shared error type (`thiserror`, kinds per design D11); verify a unit test covers `Display` output per kind

## 2. YouTube Music source

- [x] 2.1 Define `MusicSource` trait and domain types (Track, AlbumSummary, ArtistSummary, PlaylistSummary, AlbumDetail, PlaylistDetail, ArtistDetail, Page<T>, AudioStream) in `rsm-core::ytm`; verify it compiles and a `FakeSource` implements it for tests
- [x] 2.2 Implement `RustyPipeSource`: grouped search, filtered search with continuation, suggestions; verify with an `#[ignore]` network test that "daft punk" returns songs with title/artist/duration/thumbnail
- [x] 2.3 Implement album, playlist, and artist browse; verify an ignored network test fetches a known album with ordered tracks
- [x] 2.4 Implement audio stream resolution preferring itag 140 then other AAC, returning `Unavailable` otherwise; verify unit tests on stream selection over fixture stream lists

## 3. Search UI

- [x] 3.1 Build Search view: query input with empty-query guard, grouped results (songs/albums/artists/playlists) with thumbnails via `egui_extras` loaders, loading and "no results" states; verify manually that searching shows grouped results and whitespace queries do nothing
- [x] 3.2 Add filter tabs and "load more" on scroll end without duplicates; verify by scrolling a "Songs" search past the first page
- [x] 3.3 Add 300 ms debounced suggestions popup; verify typing "radioh" shows "radiohead" and selecting it searches
- [x] 3.4 Build Album, Playlist, and Artist detail views with a shared track-row widget + context menu (actions stubbed until later groups); verify opening an album shows cover, artist and ordered tracks
- [x] 3.5 Add error display with Retry for failed requests; verify by disabling network and searching

## 4. Playback engine

- [x] 4.1 Implement pure `Queue` model (play list, append, play next, remove, reorder, clear, next/prev with 3 s rule, repeat off/all/one, shuffle/unshuffle preserving current); verify with unit tests covering every playback-spec queue scenario
- [x] 4.2 Implement audio thread with rodio + symphonia playing a local M4A file with play/pause/seek/volume; verify with a test using a small generated M4A fixture (ffmpeg-generated, committed)
- [x] 4.3 Add HTTP streaming via `ChunkedHttpReader` (googlevideo `range=` chunks), resolve stream URLs with yt-dlp at play time with an expiry-aware cache, and re-resolve once on 403; verify an ignored network test plays and seeks a real track
- [x] 4.4 Implement `Player` service combining queue + audio: auto-advance, skip-with-error on unavailable stream, prefer downloaded file when present, expose state snapshot, request UI repaint on change; verify tests with `FakeSource` and a fake audio sink

## 5. Player UI and OS integration

- [x] 5.1 Build bottom Now-Playing panel (artwork, title, artist, play/pause, prev/next, seek slider, elapsed/duration, volume + mute, shuffle, repeat); verify each control manually against the playback spec scenarios
- [x] 5.2 Build Queue panel with remove, drag-reorder (`egui_dnd`), clear; wire context-menu "Play", "Play next", "Add to queue" and album/playlist "Play"; verify "Play next" ordering manually
- [x] 5.3 Integrate `souvlaki` media controls (MPRIS on Linux) for play/pause/next/prev and metadata; verify with `playerctl play-pause` and `playerctl metadata` while the window is unfocused

## 6. Library persistence and playlists

- [x] 6.1 Add SQLite DB with migrations (tables per design D8) in the data dir; verify a test opens a temp DB and migrations apply idempotently
- [x] 6.2 Implement settings store and persist volume; verify a test that volume survives reopening the DB
- [x] 6.3 Implement playlist repository (create/rename/delete with name validation, add with duplicate detection, remove, transactional reorder, list, get) and track metadata upsert; verify unit tests for each playlists-spec scenario
- [x] 6.4 Implement import of remote album/playlist as snapshot playlist; verify a test with `FakeSource` creates an ordered playlist named after the album
- [x] 6.5 Implement history recording after 30 s of playback with 200-row trim; verify unit tests for ordering and trimming
- [x] 6.6 Build Library UI: playlist list, create/rename/delete (with confirm dialog), playlist detail with drag-reorder, Play / Shuffle / start-from-track, "Add to playlist" submenu with duplicate confirm, "Save as playlist" on album/playlist views, Recently played; verify restart persistence and offline listing manually

## 7. Downloads

- [x] 7.1 Implement filename sanitization and collision-suffix logic; verify unit tests for unsafe characters and existing files
- [x] 7.2 Implement `YtDlpBackend` (audio-only M4A into temp dir, progress parsing, `--ignore-config` on all yt-dlp calls including stream resolution, yt-dlp probe plus JS-runtime detection with a warning, yt-dlp version exposed for Settings and update hint in yt-dlp errors) and `lofty` tagging (title/artist/album/cover) before moving to the final path; verify an ignored network test downloads a track that has the tags and plays in the player
- [x] 7.3 Implement download `Manager`: semaphore concurrency (configurable, default 3), job states, hidden-progress-bar polling, cancel (abort + temp cleanup) and retry, collection download skipping already-downloaded tracks; verify unit tests with a fake download backend, including cancel-at-50% leaving no file in the download dir
- [x] 7.4 Implement downloaded-file verification on startup and before play, clearing missing entries; verify a test deleting a file clears its row
- [x] 7.5 Build Downloads panel (states, %, cancel, retry), Downloads library view with delete, downloaded badges on track rows, "Download"/"Download all" actions, yt-dlp-missing notice and JS-runtime-missing warning, and Settings for download folder/concurrency/yt-dlp path (showing the detected yt-dlp version); verify manually that a downloaded track plays with network disabled

## 8. Integration checks

- [x] 8.1 Run `cargo test --workspace` and `cargo clippy --workspace -- -D warnings`; verify both pass
- [x] 8.2 Run an end-to-end manual pass on Linux: search → play → queue → add to playlist → restart → play playlist → download all → go offline → play downloaded; verify each step matches the specs
- [x] 8.3 Build `cargo build --release -p rs-music` and add `ch.compile.rs-music.desktop`; verify the release binary launches and plays a track
