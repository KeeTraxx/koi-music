# Tasks

## 1. Prepare the public repository

- [x] 1.1 Replace commercial content in the stream/download network tests (`ytdlp.rs`, `downloads/mod.rs`, `audio.rs`, `http_reader.rs`) with one freely licensed video behind a shared constant, and rephrase search-only tests away from specific commercial songs (design D8); verify the license on the video's YouTube page, `cargo test --workspace -- --ignored` passes, and `git grep -nE "dQw4w9WgXcQ|around the world"` finds nothing
- [x] 1.2 Add the "not affiliated with, endorsed by, or sponsored by Google or YouTube" disclaimer to the README next to the personal-use note; verify it renders in the README
- [x] 1.3 Set the repository-local git email to `5426590+KeeTraxx@users.noreply.github.com`, squash the history into one initial commit authored with it, and force-push `main` (design D8); verify `git log --format='%an <%ae>'` shows only the noreply address and `gh api repos/KeeTraxx/koi-music/commits --jq '.[].commit.author.email'` matches
- [x] 1.4 With the user's explicit go-ahead, make `KeeTraxx/koi-music` public; verify `gh repo view KeeTraxx/koi-music --json visibility` reports `PUBLIC`

## 2. Portable code fixes

- [x] 2.1 Avoid Windows-reserved names in `fsname::sanitize` (design D6); verify new unit tests for `CON`, `nul.m4a`-style stems, `COM1`/`LPT9`, mixed case, and that normal names are unchanged
- [ ] 2.2 Spawn every yt-dlp process (shared builder plus the `--version` and `-v` probes) with `CREATE_NO_WINDOW` on Windows (design D4); verify `cargo test -p koi-core` passes on Linux and the Windows CI job (group 3) compiles it
- [x] 2.3 Hide the console for Windows release builds with `windows_subsystem = "windows"` (design D3); verify Linux debug and release builds are unchanged
- [ ] 2.4 Replace the media tokio task with a UI-thread `MediaSession` created with the window handle from the eframe creation context and updated in `App::logic` (design D5); verify on Linux with `playerctl -l` (lists `koi_music`), `playerctl -p koi_music metadata` while a track plays, and `playerctl -p koi_music play-pause` toggling playback with the window unfocused

## 3. Continuous integration

- [x] 3.1 Add `.github/workflows/ci.yml` with the Linux + Windows matrix running clippy (`-D warnings`) and `cargo test --workspace` (design D1); verify locally that the same commands pass on Linux and the workflow file passes `actionlint` (or a YAML parse)
- [ ] 3.2 Push and get both CI jobs green, fixing any Windows build or test failures (native dependencies per the design's risk mitigations, path or line-ending issues in tests); verify with `gh run list` / `gh run view` showing a successful run on both platforms

## 4. Release and documentation

- [x] 4.1 Add a `LICENSE` file with the GPL-3.0 text; verify it matches the `license = "GPL-3.0-or-later"` field in `Cargo.toml`
- [ ] 4.2 Add `.github/workflows/release.yml` (tag `v*` → Windows release build → zip with `koi-music.exe`, `README.md`, `LICENSE` → `gh release create`) (design D2); verify the workflow file parses and the packaging steps succeed in a `workflow_dispatch` run that skips publishing
- [x] 4.3 Document Windows in the README: download the release zip, install yt-dlp and Deno via `winget`, the SmartScreen prompt, data and download locations, and the "CI-tested, not manually verified" status; verify the commands and paths match design D7 and the `platform-support` spec

## 5. Integration

- [ ] 5.1 With the user's go-ahead, push tag `v0.1.0` and check the published release: `gh release view v0.1.0` lists `koi-music-0.1.0-windows-x86_64.zip`, the zip contains `koi-music.exe`, `README.md` and `LICENSE`, and the asset downloads without authentication (`curl -fLO` on its public URL)
- [ ] 5.2 Run `cargo test --workspace` and `cargo clippy --workspace --all-targets -- -D warnings` locally and confirm the latest CI run is green on Linux and Windows

## Workflow follow-up

- Manual Windows verification (playback, media keys, fonts, downloads, no console window) remains open by decision; run it when a Windows machine is available.
- Archive the change with `/opsx:archive` after review.
