# Design

## Context

Greenfield project: the repository has only OpenSpec scaffolding. See proposal.md for motivation and the specs under `specs/` for required behavior.

Decisions already made with the user:
- **Pure-Rust desktop GUI with egui** (via `eframe`); keep the stack as Rust as possible (no webview, no JS).
- YouTube Music metadata in **native Rust** with `rustypipe`. Stream URLs and downloads come from **yt-dlp** (decided during implementation, see D4/D9: rustypipe 0.11.4 can only fetch the first ~1 MB of streams today; the user preferred `rustypipe-downloader`, which is affected the same way).
- App identifier: **`ch.compile.rs-music`**.

Environment observed: `rustup` (stable, toolchain needs `rustup toolchain install stable` if `rustc` is missing), `ffmpeg`, and `yt-dlp` are installed via Nix; `mpv` is not. Linux is the primary development target.

## Goals / Non-Goals

**Goals:**
- Keep all YouTube, audio, storage, and download logic in a UI-independent library crate so it can be tested without a window.
- Single source of truth for player, queue, library, and download state in the core; the egui app renders snapshots of it each frame.
- Work offline for playlists, history, and downloaded tracks.

**Non-Goals:**
- Google account login, syncing with a user's YouTube Music library, likes, or uploads.
- Video playback, lyrics, radio/autoplay recommendations, equalizer, crossfade or gapless playback.
- Mobile/web targets; installers, signing, or auto-update. A release binary plus a Linux `.desktop` file is enough.
- Bundling, downloading or auto-updating yt-dlp or a JavaScript runtime; they are user-provided.
- Windows as a release target (follow-up change).

## Decisions

### D1. Workspace layout
```
Cargo.toml              (workspace, license GPL-3.0-or-later)
crates/rsm-core/        library: ytm source, player, queue, library db, downloads
crates/rs-music/        eframe binary: UI only, depends on rsm-core
```
*Alternative:* single crate — rejected; a UI-free core keeps tests fast and headless.

### D2. UI: egui via eframe
`eframe` (wgpu or glow backend, default features) with `NativeOptions { viewport: app_id("ch.compile.rs-music") }`. Immediate-mode UI: each frame reads cheap state snapshots from core services (behind `Arc<RwLock<_>>` / `watch` channels) and sends user intents as commands. Backend tasks call `egui::Context::request_repaint()` when state changes, so the UI is idle when nothing happens.
Supporting crates: `egui_extras` with `image` + `http` loaders (`install_image_loaders`) for thumbnails from URLs, `egui_dnd` for drag-reorder in queue/playlists. Layout: left side panel (Search, Library: Playlists / Downloads / Recently played, Settings), central panel for the active view, right collapsible Queue panel, bottom Now-Playing panel, Downloads window/panel. A shared track-row widget provides the context menu (Play, Play next, Add to queue, Add to playlist ▸, Download).
*Alternatives:* Tauri (webview + JS) — rejected per "as Rust as possible"; iced/Slint — viable, egui chosen by the user.

### D3. Async runtime bridge
A multi-threaded `tokio` runtime is created in `main` before `eframe::run_native` and owned by an `AppCore` handle. UI code never blocks: it spawns futures (search, browse, downloads) and receives results through channels polled each frame (`try_recv`), storing them in per-view state (`Loading | Ready(T) | Error(e)`). Search suggestions are debounced (300 ms) by tracking last-keystroke time in UI state.

### D4. YouTube Music access: `rustypipe`
Use `rustypipe` for `music_search_main`, filtered `music_search_*` with continuation, `music_search_suggestion`, `music_album`, `music_playlist`, and `music_artist`. Everything sits behind the `rsm-core::ytm::MusicSource` trait so tests use a fake source. Rustypipe's storage (visitor data, deobfuscation cache) lives in the app data dir.

**Stream resolution via yt-dlp.** Verified on 2026-10-28: rustypipe's iOS client URLs are rejected (403) after the first ~1 MB without a PO token, and all other clients fail in its deobfuscator (no upstream fix since April 2025). `audio_stream` therefore runs `yt-dlp -j --no-playlist -f "140/bestaudio[acodec^=mp4a]" <music.youtube.com URL>` and uses `url`, `filesize` and `http_headers.User-Agent` from the JSON. Every yt-dlp invocation passes `--ignore-config` so a user's yt-dlp config file (e.g. a custom `-o`, `--print` or `--embed-*`) cannot change our output format or file placement; yt-dlp plugins (e.g. PO-token providers) still load. yt-dlp ≥ 2025.11 needs a JS runtime (Deno recommended) to solve YouTube's player challenges; without one it runs in a deprecated mode with fewer formats. To limit player requests (and YouTube's "confirm you're not a bot" checks, which are IP/rate based and hit any client), resolved streams are cached per video id until their `expire` timestamp minus a margin. A bot-check failure is reported with a clear message. The pure-Rust path can be restored by swapping the resolver once rustypipe is fixed.

### D5. App directories
`directories::ProjectDirs::from("ch", "compile", "rs-music")` gives data dir (DB, rustypipe cache), cache dir (stream buffers, thumbnails), and config. Default download dir: `dirs::audio_dir()/rs-music` (fallback `data_dir/downloads`), configurable.

### D6. Audio playback: rodio + symphonia
Playback runs in `rsm-core::player` on a dedicated audio thread controlled via a command channel, using `rodio` (output) + `symphonia` (decode; features `aac`, `isomp4`). Streaming uses our own `ChunkedHttpReader` (`Read + Seek`): ranged requests over `reqwest` using googlevideo's `range=` query parameter in ~9.5 MB chunks (same as rustypipe-downloader), each body streamed into an in-memory buffer so playback starts immediately; reads block until bytes arrive. `stream-download` was dropped because its HTTP source issues an un-ranged first request, which googlevideo rejects.
Stream selection prefers **audio-only AAC/M4A** (itag 140) because symphonia has no Opus decoder; fall back to other AAC streams. Downloaded files are played from disk.
Stream URLs expire (~6 h); the URL is resolved at play time and re-resolved once on HTTP 403.
*Alternative:* libmpv — rejected: native non-Rust dependency, not installed.

### D7. Queue model
`Queue { items: Vec<QueueItem>, original_order: Option<Vec<ItemId>>, current: Option<usize>, repeat: RepeatMode, shuffle: bool }`. Each `QueueItem` has a unique id (duplicates distinguishable). Shuffle randomizes the items after the current one and stores `original_order` to restore on unshuffle. Pure logic, fully unit tested.

### D8. Persistence: SQLite via `rusqlite` (bundled)
Single DB `<data_dir>/library.db`, migrations at startup (`rusqlite_migration`). Tables:
- `tracks(video_id PK, title, artists_json, album, album_id, duration_s, thumbnail_url, updated_at)`
- `playlists(id PK, name, created_at, updated_at, source_id NULL)`
- `playlist_items(id PK, playlist_id FK, video_id FK, position)` — renumbered transactionally on reorder
- `downloads(video_id PK, path, size_bytes, completed_at)`
- `history(id PK, video_id, played_at)` — trimmed to 200 rows on insert
- `settings(key PK, value_json)` — volume, download dir, concurrency
DB access runs on a blocking thread (`tokio::task::spawn_blocking`) behind a `Library` service.

### D9. Downloads: yt-dlp + lofty
`rsm-core::downloads::Manager` owns the job queue; the actual fetch is a `DownloadBackend` trait so tests use a fake.
- **Backend (`YtDlpBackend`):** `yt-dlp --ignore-config --no-playlist -f "140/bestaudio[acodec^=mp4a]" --newline --progress-template "download:rsm %(progress.downloaded_bytes)s %(progress.total_bytes)s %(progress.total_bytes_estimate)s" --print after_move:filepath -o <download_dir>/.rs-music-tmp/<video_id>.%(ext)s <url>`, spawned with `tokio::process` (`kill_on_drop`). Progress lines are parsed from stdout; the final path comes from the `after_move:filepath` print.
- **Tags (Rust):** after download, `lofty` writes title, artist, album and cover art (thumbnail fetched with `reqwest`) from our `Track` metadata, so tags match what the app shows.
- **Placement:** move the tagged file to `<download_dir>/<Artist> - <Title>.<ext>` (sanitized, collision suffix), then insert the `downloads` row. Downloading into a temp dir and moving means the visible folder never has partial files.
- **Concurrency:** a `tokio::Semaphore` (default 3 permits); job states queued / downloading % / completed / failed / cancelled; progress published ≤ 4×/s.
- **Cancellation:** abort the job task (drops and kills the yt-dlp child) and remove its temp files; retry re-enqueues.
- **Probe:** `yt-dlp --version` at startup; if missing, playback of non-downloaded tracks and downloads are disabled with an explanatory message (with install help). The JS runtime is detected from the `JS runtimes:` line of `yt-dlp -v` output; if none is found, the app shows a warning but keeps working. Path configurable in settings. ffmpeg is optional (yt-dlp uses it for an M4A container fixup when present).
- **User-provided tools:** yt-dlp and the JS runtime are installed and updated by the user (README documents how); rs-music never bundles, downloads or updates them. The detected yt-dlp version is shown in Settings, and `Unavailable`/`BotCheck` errors from yt-dlp include a hint to update yt-dlp.
- On startup and before playing a "downloaded" track, file existence is checked; missing files clear the row.
*Alternative:* `rustypipe-downloader` (user's original preference) — blocked by the same ~1 MB stream limit; revisit when rustypipe is fixed.

### D10. OS media integration
`souvlaki` for media keys and now-playing metadata (MPRIS on Linux; SMTC on Windows needs the window handle from eframe; macOS MPNowPlaying). Events are forwarded into the player command channel.

### D11. Error handling
`rsm-core` uses a `thiserror` enum (`Network`, `Unavailable`, `NotFound`, `Db`, `Io`, `Cancelled`, `MissingTool`, `Invalid`, `Audio`, `BotCheck`). The UI shows `Network` errors with a Retry button and others as inline messages/toasts.

## Risks / Trade-offs

- [YouTube changes break rustypipe extraction] → Isolate behind `MusicSource`; pin and bump `rustypipe`; clear "Unavailable" errors.
- [Itag 140 missing for some tracks] → Fall back to any AAC stream; if only Opus exists, report as unplayable (Opus support is a follow-up).
- [yt-dlp (+ JS runtime) is a non-Rust, user-provided runtime dependency] → Accepted (user decision, 2026-10-28) after comparing alternatives (embedded QuickJS with yt-dlp's EJS solver, rustypipe-botguard, Invidious/Piped APIs): yt-dlp keeps pace with YouTube fastest and rustypipe's stream code is dormant. Isolated behind the stream resolver and `DownloadBackend`; detected at startup; `--ignore-config` keeps invocations predictable.
- [User's yt-dlp is outdated] → Most likely cause of sudden "Unavailable" errors; show the version in Settings and an "update yt-dlp" hint in errors.
- [YouTube "confirm you're not a bot" checks] → IP/rate based, affect any client; cache resolved URLs until expiry, resolve only at play time, show a clear error. README recommends installing a yt-dlp PO-token provider plugin (plugins load despite `--ignore-config`). Follow-up: optional cookies-from-browser setting.
- [GPL-3.0 dependencies] → Project licensed GPL-3.0-or-later.
- [Stream URL expiry mid-queue] → Resolve at play time; retry once on 403.
- [Downloading violates YouTube ToS] → Personal-use app; documented in README.
- [No gapless/crossfade] → Accepted.

## Migration Plan

Not applicable (greenfield).
