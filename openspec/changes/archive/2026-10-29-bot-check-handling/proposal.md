# Proposal

## Why

When YouTube answers yt-dlp with "Sign in to confirm you're not a bot" (an IP-reputation/rate block that affects the whole network, not one track), koi-music treats it like a single unplayable track:

- **Playback** skips to the next queue item, which hits the same block: one error toast and one more YouTube request per remaining track, until the queue is exhausted.
- **Downloads** keep starting the remaining jobs ("Download all" on 50 tracks means 50 failing yt-dlp runs).
- **The message** is long and partly wrong: it appends "If this keeps happening, update yt-dlp", which does not help against an IP block.

Each extra request makes the block more likely to persist. The user decided (2026-10-29): show one clear "try again later" message, stop playback, and stop all downloads, with no automatic resume.

## What Changes

- A bot check produces one clear message: YouTube is temporarily limiting requests from this network, try again later. The "update yt-dlp" hint is no longer appended to bot-check errors (it stays for "unavailable" errors).
- **Playback:** a bot check stops playback instead of advancing through the queue. The current track stays selected; pressing play retries it. Genuinely unavailable tracks (removed, region-blocked) still skip to the next item as today.
- **Downloads:** a bot check in any download stops all queued and running downloads. They are marked failed with the bot-check message and no partial files remain. Nothing resumes automatically; the user retries when they want to. One notice is shown, not one per download.
- README: describe the new behavior next to the existing bot-check note.
- Publish release v0.1.1 after this change (also ships the Windows rendering fix, the startup error reporting and the song-duration fix already on `main`).

Out of scope: automatic cooldown or resume, pacing bulk downloads, cookies (dropped earlier), detecting bot checks in rustypipe metadata requests (search/browse do not go through yt-dlp).

## Capabilities

### New Capabilities

None.

### Modified Capabilities
- `playback`: new requirement – a YouTube bot check stops playback with a "try again later" message instead of skipping through the queue.
- `downloads`: new requirement – a YouTube bot check stops all queued and running downloads, with one "try again later" notice and no automatic resume.

## Impact

- **Code:** `crates/koi-core/src/ytdlp.rs` (message, no update hint for bot checks), `crates/koi-core/src/player.rs` (stop instead of advance on `BotCheck`), `crates/koi-core/src/downloads/mod.rs` (stop all jobs, one notice), `crates/koi-music/src/app.rs` (show the downloads notice once), test fakes in `ytm/fake.rs` and `downloads/mod.rs`.
- **Docs:** README bot-check paragraph.
- **Release:** version bump to 0.1.1, tag `v0.1.1`.
- No new dependencies.
