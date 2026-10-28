# koi-music

A lightweight desktop player for YouTube Music, written in Rust with [egui](https://github.com/emilk/egui).
No account, no browser tab, no ads.

- Search songs, albums, artists and playlists; browse albums, playlists and artists
- Audio-only streaming with a play queue, shuffle, repeat, seek and volume
- Local playlists (stored in SQLite), saving YouTube Music albums/playlists as local playlists, "Recently played"
- Downloads as tagged M4A files (title, artist, album, cover) that play offline
- Media keys and now-playing info via MPRIS (Linux)

Linux is the primary platform. macOS and Windows may work but are not tested yet.

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

Examples:

```sh
# Arch Linux
sudo pacman -S yt-dlp deno ffmpeg
# Debian/Ubuntu: distro yt-dlp packages are often too old; use the official binary or pipx
pipx install yt-dlp && curl -fsSL https://deno.land/install.sh | sh
# Nix
nix profile install nixpkgs#yt-dlp nixpkgs#deno nixpkgs#ffmpeg
```

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

## Project layout

- `crates/koi-core`: UI-independent library (YouTube Music access, player, queue, library DB, downloads)
- `crates/koi-music`: the egui application
- `openspec/`: specifications and design notes

## License

GPL-3.0-or-later (koi-music uses the GPL-3.0 [rustypipe](https://codeberg.org/ThetaDev/rustypipe) crate).
