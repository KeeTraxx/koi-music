# Design

## Context

See proposal.md for motivation and specs/ for the required behavior. Current state relevant to Windows:

- No platform-specific code (`cfg(windows)`) exists. All crates are portable in principle: audio via cpal (WASAPI on Windows), fonts via eframe's system-font fallback (DirectWrite on Windows), paths via `directories`/`dirs`.
- `koi_core::media::start` creates the `souvlaki` controls with `hwnd: None` and syncs them from a tokio task. That works for MPRIS, but Windows System Media Transport Controls (SMTC) need the app's window handle.
- yt-dlp is spawned with `tokio::process::Command` from a GUI process.
- Native C code in the dependency tree: bundled SQLite (`rusqlite`), QuickJS (`rquickjs-sys`, via rustypipe), `aws-lc-sys` and `ring` (rustls). `rquickjs-sys` ships prebuilt bindings for `x86_64-pc-windows-msvc`.
- No CI exists; the repository is private on GitHub (`KeeTraxx/koi-music`) and has one commit, authored with a personal email address.
- The ignored network tests stream or download commercial recordings (`dQw4w9WgXcQ`, "daft punk around the world").
- Verification is CI-only (user decision): there is no Windows machine to test on by hand.

## Goals / Non-Goals

**Goals:**
- Windows (x86_64-pc-windows-msvc) builds, passes `cargo test` and `clippy -D warnings` in CI, alongside Linux.
- A tag push produces a downloadable Windows zip on a GitHub release.
- The Windows-specific pitfalls that CI cannot catch are handled by construction: console windows, SMTC window handle, reserved file names.

**Non-Goals:**
- Manually verified Windows runtime behavior (playback, media keys, fonts, downloads). These are implemented to the best of our knowledge but only checked by compiling and unit tests.
- Running the `#[ignore]` network tests in CI (they depend on YouTube, yt-dlp and bot checks, so they would be flaky).
- Logging to a file on Windows (the release binary has no console, so logs are not visible there).
- `windows-gnu`, ARM64, macOS.

## Decisions

### D1. CI: one GitHub Actions workflow, Linux + Windows matrix
`.github/workflows/ci.yml` runs on branch pushes and pull requests (not on tag pushes: a tag points at a commit already tested on its branch, and the release workflow builds it) with a matrix of `ubuntu-latest` and `windows-latest`: install the toolchain from `rust-toolchain.toml`, install ALSA headers on Linux only, cache with `Swatinem/rust-cache`, then `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace`. CI runners have no audio device; the audio tests already skip in that case.
*Alternative:* cross-compiling Windows from Linux (`cargo-xwin`) – rejected: it would not run the tests on Windows, and C dependencies are easier on a native MSVC runner.

### D2. Release: tag-triggered workflow using the `gh` CLI
`.github/workflows/release.yml` runs on tags `v*` on `windows-latest`: `cargo build --release --locked -p koi-music`, zip `koi-music.exe` with `README.md` and `LICENSE` into `koi-music-<version>-windows-x86_64.zip`, then `gh release create` (preinstalled on runners, authenticated with `GITHUB_TOKEN`) with the zip attached. A `LICENSE` file with the GPL-3.0 text is added to the repository, because distributing GPL binaries should ship the license text.
*Alternative:* third-party release actions (e.g. `softprops/action-gh-release`) – not needed when `gh` is available, and one less external dependency.

### D3. No console window for release builds
`#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]` in `koi-music`'s `main.rs`. Debug builds keep the console so `cargo run` still shows logs.

### D4. Child processes without console windows
On Windows, every yt-dlp invocation (probe, stream resolution, download) is spawned with the `CREATE_NO_WINDOW` creation flag (via tokio's `Command::creation_flags`). Without it, a console window would flash up each time a GUI process starts a console program. This is set once in the shared command builder in `ytdlp.rs` (including the `--version` and `-v` probes, which currently build their own `Command`). yt-dlp's own children (Deno, ffmpeg) inherit its hidden console.

### D5. Media controls created on the UI thread with the window handle
`media::start` changes to `media::MediaSession::new(player, window_handle)`, called from the eframe app creator with the Win32 HWND taken from `CreationContext` (via `raw-window-handle`; `None` on other platforms). The session is owned by the app and updated from `eframe::App::logic`, which runs on the UI thread on every repaint, including when the window is hidden. The player's notifier already requests a repaint on every state change. Media key events still go straight from souvlaki's callback to `Player::send`.
This replaces the tokio sync task on all platforms, so there is one code path. Linux MPRIS must be re-checked with `playerctl` after the change.
*Alternative:* `cfg(windows)`-only UI-thread path and keep the tokio task on Linux – rejected: two code paths for the same feature, and the Windows one would only ever be compile-tested.

### D6. Windows-reserved file names
`fsname::sanitize` appends `_` to a stem whose part before the first `.` equals (case-insensitively) `CON`, `PRN`, `AUX`, `NUL`, `COM1`–`COM9` or `LPT1`–`LPT9`, and still trims trailing dots and spaces (already done). It applies on all platforms, so files copied from Linux to Windows stay valid. Unit tests cover the reserved names.

### D7. Paths and tools need no special code
- `ProjectDirs::from("ch", "compile", "koi-music")` resolves to `%APPDATA%\compile\koi-music\data` (library) and `%LOCALAPPDATA%\compile\koi-music\cache`; `dirs::audio_dir()` is the user's Music folder. This matches the spec without changes.
- `Command::new("yt-dlp")` finds `yt-dlp.exe` on `PATH` (Windows appends `.exe` when searching), so the default `yt-dlp` setting works for winget/pip installs.
- README gains a Windows section: download the release zip, `winget install yt-dlp.yt-dlp DenoLand.Deno` (ffmpeg optional), SmartScreen "More info → Run anyway", where data lives.

### D8. Publication: clean tests, noreply author, rewritten history, user-confirmed switch
- **Test content:** tests that stream or download (`ytdlp.rs`, `downloads/mod.rs`, `audio.rs`, `http_reader.rs`) use one freely licensed video (Creative Commons or public domain, e.g. an official Blender Foundation open movie upload), referenced by a single shared constant and resolved by video id rather than by search. The license is checked on its YouTube page when the id is chosen. Search-only tests may keep generic queries; they are rephrased away from specific commercial songs where they download nothing.
- **Disclaimer:** README adds "not affiliated with, endorsed by, or sponsored by Google or YouTube" next to the personal-use note.
- **Author email:** repository-local `git config user.email 5426590+KeeTraxx@users.noreply.github.com`; the existing history is squashed into one initial commit authored with that address and force-pushed before publication, so neither the personal email nor the old test content is published. Later commits are normal (no further force-pushes).
- **Visibility:** `gh repo edit KeeTraxx/koi-music --visibility public --accept-visibility-change-consequences` runs only after the user's explicit go-ahead at that step, because publication cannot be undone (clones, caches).
- **Order:** publication comes first, so CI runs on a public repository (free standard runners) and releases are public from the first tag.
*Alternative:* a fresh public repository instead of rewriting history – equivalent outcome, but loses the existing repo settings; rewriting a one-commit history is trivial.

## Risks / Trade-offs

- [Windows runtime behavior is unverified] → Accepted (user decision). CI catches build and unit-test failures; the README marks Windows as "CI-tested, not manually verified" until someone runs the checklist.
- [Native dependencies fail to build with MSVC (`aws-lc-sys` may want NASM/CMake; `rquickjs-sys`)] → The first CI run decides. Mitigations in order: install NASM in the Windows job, enable `aws-lc-sys`'s `prebuilt-nasm` feature, or switch reqwest to the `ring` crypto provider.
- [SMTC callbacks or `souvlaki` behave differently on Windows] → Compile-checked only. If media keys fail on Windows, the app keeps working; the playback spec only requires OS integration "where the platform supports it".
- [Unsigned binary triggers SmartScreen] → Documented in the README.
- [Long download paths exceed Windows' 260-character limit] → Stems are already capped at 180 characters; a deep custom download folder could still exceed it. Not addressed; a failed download shows the error.
- [Deleting a download that is currently playing fails on Windows (file locked)] → The delete shows an error toast; deleting after playback works. Accepted.
- [DMCA notices against YouTube download tools (youtube-dl, 2020)] → No commercial content in tests, disclaimer, personal-use note, no claims about circumventing protection. The risk remains for any public YouTube download tool and is accepted.
- [Publication is irreversible] → Clean history and tests first; the visibility switch needs the user's explicit go-ahead.
- [Force-push rewrites `main`] → One time only, before publication, while nobody else has cloned the repository.

## Migration Plan

No data migration. Rollback is reverting the change; the workflows can be disabled in the GitHub UI.
