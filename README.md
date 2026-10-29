# koi-music

A lightweight desktop player for YouTube Music, written in Rust with [egui](https://github.com/emilk/egui).
No account, no browser tab, no ads.

- Search songs, albums, artists and playlists; browse albums, playlists and artists
- Audio-only streaming with a play queue, shuffle, repeat, seek and volume
- Local playlists (stored in SQLite), saving YouTube Music albums/playlists as local playlists, "Recently played"
- Downloads as tagged M4A files (title, artist, album, cover) that play offline
- Media keys and now-playing info (MPRIS on Linux, system media controls on Windows)

Platforms: **Linux** (x86_64) and **Windows 10/11** (x86_64). Windows builds and tests run in CI
on every push; the Windows app has been tested by hand in a virtual machine (software rendering),
not yet on a physical PC with a GPU. Reports welcome. macOS is not supported.

> **Personal use only.** Streaming and downloading via unofficial APIs may conflict with YouTube's Terms of Service.
> koi-music is an independent project. It is not affiliated with, endorsed by, or sponsored by Google or YouTube.

## Runtime requirements

koi-music does **not** ship or download any of these. Install them yourself and keep yt-dlp up to date:

| Tool | Needed for | Notes |
|---|---|---|
| [yt-dlp](https://github.com/yt-dlp/yt-dlp) | Streaming and downloading | Must be on `PATH`, or set its path in *Settings*. Without it only downloaded tracks play. |
| A JavaScript runtime, e.g. [Deno](https://deno.com) | yt-dlp's YouTube support | Required by yt-dlp ≥ 2025.11. Without one, yt-dlp works in a degraded mode and koi-music shows a warning. |
| ffmpeg (optional) | M4A container fix-up | yt-dlp uses it when present. |
| A D-Bus session (optional) | Media keys / MPRIS | Present on normal Linux desktops. |
| fontconfig and fonts for other scripts (optional) | Titles in Japanese, Chinese, Korean, Arabic, … | koi-music bundles Latin and emoji fonts and falls back to installed system fonts, e.g. Noto Sans CJK. |

koi-music runs yt-dlp with `--ignore-config`, so your personal yt-dlp config file does not affect it.
yt-dlp **plugins** still load. If you often see *"Sign in to confirm you're not a bot"*, installing a
PO-token provider plugin (for example [bgutil-ytdlp-pot-provider](https://github.com/Brainicism/bgutil-ytdlp-pot-provider))
can help. These checks depend on your IP address's reputation and affect every YouTube client.

When YouTube shows this check, koi-music says that YouTube is limiting requests and to try again later.
Playback stops on the current track instead of skipping through the queue (press play to try again), and
all queued and running downloads stop and show as failed (use *Retry* later). Nothing resumes
automatically, so the app does not keep sending requests while the block lasts. Downloaded tracks keep
playing.

Examples:

```sh
# Arch Linux
sudo pacman -S yt-dlp deno ffmpeg
# Debian/Ubuntu: distro yt-dlp packages are often too old; use the official binary or pipx
pipx install yt-dlp && curl -fsSL https://deno.land/install.sh | sh
# Nix
nix profile install nixpkgs#yt-dlp nixpkgs#deno nixpkgs#ffmpeg
```

## Windows

1. Download `koi-music-<version>-windows-x86_64.zip` from the
   [releases page](https://github.com/KeeTraxx/koi-music/releases) and extract it anywhere.
2. Install yt-dlp and Deno (ffmpeg is optional):
   ```powershell
   winget install yt-dlp.yt-dlp DenoLand.Deno
   winget install Gyan.FFmpeg   # optional
   ```
   Open a new terminal or sign out and in so the updated `PATH` is picked up. Keep yt-dlp current
   with `winget upgrade yt-dlp.yt-dlp`.
3. Start `koi-music.exe`. The binary is not code-signed, so Windows SmartScreen may show
   "Windows protected your PC": choose **More info → Run anyway**.

If yt-dlp is installed somewhere not on `PATH`, set the full path to `yt-dlp.exe` in *Settings*.

On Windows, koi-music renders with Direct3D. Without a GPU driver (for example in a virtual machine with
a display-only graphics driver) it falls back to Windows' built-in software renderer, which works but
uses more CPU. If the app cannot start, it shows a message with the reason and the log file location.

## Building

Requirements: Rust (the version in `rust-toolchain.toml`, installed automatically by rustup), `pkg-config`
and the ALSA development files.

```sh
# Arch Linux
sudo pacman -S --needed pkgconf alsa-lib
# Debian/Ubuntu
sudo apt install pkg-config libasound2-dev

cargo run --release -p koi-music
```

With Nix, `nix-shell` provides all build and runtime dependencies (see `shell.nix`):

```sh
nix-shell --run 'cargo run -p koi-music'
```

### Tests

```sh
cargo test --workspace                 # offline tests
cargo test --workspace -- --ignored    # also network tests (need yt-dlp and internet)
cargo clippy --workspace --all-targets -- -D warnings
```

## Desktop entry

A release build plus the desktop file makes koi-music show up in application launchers:

```sh
cargo build --release -p koi-music
install -Dm755 target/release/koi-music ~/.local/bin/koi-music
install -Dm644 ch.compile.koi-music.desktop ~/.local/share/applications/ch.compile.koi-music.desktop
```

## Files

| What | Where (Linux) |
|---|---|
| Library (playlists, history, downloads, settings) | `~/.local/share/koi-music/library.db` |
| YouTube client cache | `~/.local/share/koi-music/rustypipe/` |
| Downloads (default, configurable) | `~/Music/koi-music/` |
| Log of the last run | `~/.cache/koi-music/koi-music.log` |

On Windows the library and client cache live in `%APPDATA%\compile\koi-music\data\`, other
caches and the log (`koi-music.log`) in `%LOCALAPPDATA%\compile\koi-music\cache\`, and downloads
go to `koi-music` in your Music folder.

## Project layout

- `crates/koi-core`: UI-independent library (YouTube Music access, player, queue, library DB, downloads)
- `crates/koi-music`: the egui application
- `openspec/`: specifications and design notes

## License

GPL-3.0-or-later (koi-music uses the GPL-3.0 [rustypipe](https://codeberg.org/ThetaDev/rustypipe) crate).
