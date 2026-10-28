# Proposal

## Why

There is no lightweight, open, native desktop client for YouTube Music written in Rust. Inspired by YMusic (an Android YouTube Music client), this project builds a desktop app that lets users search, stream, organize, and download music from YouTube Music without a browser tab, ads, or a Google account.

## What Changes

- New greenfield native desktop application (`rs-music`, app id `ch.compile.rs-music`) written entirely in Rust with an egui (eframe) GUI.
- Search YouTube Music for songs, albums, artists, and playlists, with paginated results.
- Stream audio-only playback with a play queue, play/pause/seek/next/previous, volume, shuffle, and repeat.
- Local playlists: create, rename, delete, add/remove/reorder tracks, persisted across restarts; play remote YouTube Music albums/playlists from search results and save them as local playlists.
- Download tracks (and whole playlists) as audio files with embedded metadata and cover art, with progress reporting; downloaded tracks play offline.
- No account login: all access is anonymous.

## Capabilities

### New Capabilities

- `music-search`: Querying YouTube Music and presenting songs, albums, artists, and playlists, including browsing an album/playlist's tracks.
- `playback`: Audio streaming and the play queue: transport controls, seeking, volume, shuffle/repeat, and now-playing state.
- `playlists`: User-owned local playlists and their persistence, plus importing remote albums/playlists.
- `downloads`: Saving tracks to disk as tagged audio files, download progress and failure handling, and offline use of downloaded tracks.

### Modified Capabilities

None (no existing specs).

## Impact

- New codebase: Cargo workspace with a UI-independent core library (`crates/rsm-core`) and an eframe/egui app (`crates/rs-music`).
- New dependencies: `eframe`/`egui` (+ `egui_extras` image loading), `rustypipe` (YouTube Music search/browse metadata), `rodio`/`symphonia` (audio), `lofty` (audio tags), `rusqlite`, `souvlaki` (media keys), `tokio`, `reqwest`.
- Runtime dependencies, **installed by the user and never bundled or downloaded by rs-music**: the `yt-dlp` binary on `PATH` (path configurable), used only to resolve audio stream URLs and to download audio, plus a JavaScript runtime that yt-dlp uses for YouTube (Deno recommended; required for full YouTube support since yt-dlp 2025.11). rustypipe 0.11.4 can no longer fetch complete streams (only ~1 MB before YouTube rejects them; its deobfuscator is outdated and the project has had no stream-related changes since mid-2025), and yt-dlp is the most actively maintained extractor, so this is a deliberate, isolated exception to the pure-Rust goal. Playback of non-downloaded tracks and downloads are disabled with a clear message when yt-dlp is missing; a missing JS runtime only produces a warning.
- Licensing: `rustypipe` is GPL-3.0, so `rs-music` is licensed GPL-3.0-or-later.
- Platform: Linux first; macOS and Windows supported by the chosen crates but not a gating target for this change. Windows as a release target is a follow-up change.
- External system: relies on YouTube's unofficial InnerTube API and on yt-dlp keeping up with YouTube; breakage and YouTube's "confirm you're not a bot" checks are accepted risks (see design.md).
- Legal: downloading may conflict with YouTube's Terms of Service; the app is intended for personal use.
