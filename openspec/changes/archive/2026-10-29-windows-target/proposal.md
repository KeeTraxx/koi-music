# Proposal

## Why

koi-music is Linux-only in practice: nothing is built or tested on Windows, OS media controls are registered without the window handle Windows needs, and there is no binary a Windows user could download. Supporting Windows was one of the reasons for keeping yt-dlp as a user-installed tool (2026-10-28) and was deferred as a follow-up of `youtube-music-player`. A downloadable binary is only useful if the repository is public (release assets of private repositories are visible to collaborators only), so this change also prepares the repository for publication.

## What Changes

- Make the codebase build and pass its tests on Windows (x86_64, MSVC toolchain), checked by GitHub Actions on every push and pull request (Linux is checked in the same workflow).
- Publish a prebuilt `koi-music.exe` (zipped) on GitHub releases when a version tag is pushed. No installer, no code signing (Windows SmartScreen will warn on first start).
- Windows-native app behavior: no console window for the release binary; OS media controls (System Media Transport Controls) wired with the app's window handle; standard Windows locations for data (`%APPDATA%`) and downloads (Music folder).
- Download file names avoid Windows-reserved names (`CON`, `NUL`, `COM1`, …) in addition to the characters already replaced.
- README: Windows install steps (download the release, install yt-dlp and Deno, e.g. via `winget`), SmartScreen note.
- Prepare the repository for publication, then make it public:
  - Network tests that stream or download use freely licensed content instead of commercial recordings (youtube-dl's 2020 DMCA takedown cited tests that downloaded copyrighted songs).
  - README states that koi-music is not affiliated with Google or YouTube.
  - Commits are authored with the GitHub noreply address (`5426590+KeeTraxx@users.noreply.github.com`); the existing history is squashed and rewritten before publication so the old email and test content are not public.
  - The visibility switch to public happens only after the user's explicit go-ahead.
- Verification is CI-only: Windows builds and unit tests run in CI; playback, media keys, fonts and downloads on real Windows are not manually verified in this change.

Out of scope: installer/MSI, Start-menu integration, code signing, auto-update, ARM64 Windows, macOS, bundling yt-dlp or Deno (still user-installed), prebuilt Linux binaries.

## Capabilities

### New Capabilities
- `platform-support`: Which platforms koi-music supports, how it is built and distributed per platform, and platform conventions it follows (data/download locations, console window, user-installed tools).

### Modified Capabilities
- `downloads`: "Download a track as an audio file" gains a scenario for file names that are reserved on Windows.

## Impact

- **Code:** `crates/koi-core/src/media.rs` (create media controls with the window handle; Windows needs them on the UI thread), `crates/koi-music/src/main.rs` (pass the handle, hide the console on Windows release builds), `crates/koi-core/src/fsname.rs` (reserved names). Possibly small `cfg(windows)` fixes surfaced by the first CI run.
- **New files:** `.github/workflows/ci.yml` (Linux + Windows build, test, clippy), `.github/workflows/release.yml` (tag → Windows zip on a GitHub release).
- **Dependencies:** `raw-window-handle` (already transitive via eframe) for the Win32 window handle; no new runtime dependencies.
- **Tests:** ignored network tests in `ytdlp.rs`, `downloads/mod.rs`, `audio.rs`, `http_reader.rs` switch to freely licensed content.
- **External systems:** `KeeTraxx/koi-music` becomes public (GitHub Actions on standard runners is then free); history is force-pushed once before publication; GitHub releases are publicly downloadable.
- **Risk:** native C dependencies (`rusqlite` bundled SQLite, `rquickjs-sys` via rustypipe, `aws-lc-sys` via rustls) must compile with MSVC; the first CI run is the real check.
