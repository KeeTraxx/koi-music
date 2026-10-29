# Design

## Context

See proposal.md for motivation and the delta specs for required behavior. Current code:

- `ytdlp::classify_failure` already recognizes the bot check (`ErrorKind::BotCheck`, matching "not a bot" / "sign in to confirm" in yt-dlp's stderr) and then appends `UPDATE_HINT` to both `Unavailable` and `BotCheck` errors.
- `Player::fail_current` handles every stream error the same way: report `Cannot play "<title>": <error>`, then `queue.advance()` and start the next track, stopping only after every queue item failed in a row.
- `downloads::Manager` marks a failed job `Failed(e.to_string())` and leaves the rest of the queue running; jobs do not know about each other. Cancelling uses `abort` + temp cleanup with a per-attempt run token, so late results of an aborted attempt are ignored.
- The UI shows the player's `last_error` as a toast (deduplicated by `seq`); download failures appear only in the Downloads view.

## Goals / Non-Goals

**Goals:**
- One request after a bot check, not N: neither playback nor downloads start further yt-dlp calls on their own.
- One clear message per bot-check event, in playback and in downloads.

**Non-Goals:**
- Cooldown timers, automatic resume, or a global "YouTube blocked" state shared by player and downloads. A bot check in playback does not stop downloads and vice versa; each stops its own pipeline. (Both usually hit the block at the same time anyway, and the user can retry either.)
- Detecting blocks in rustypipe metadata requests (search, browse).

## Decisions

### D1. One message text, no update hint for bot checks
`classify_failure` keeps the update hint for `Unavailable` only. The `BotCheck` message becomes: "YouTube is temporarily limiting requests from your network (it asks to confirm you're not a bot). Try again later." The `ErrorKind::BotCheck` display prefix is dropped for this kind (the message is self-explanatory), so the UI shows exactly that sentence.
*Alternative:* keep the "or from another network" advice – it suggests VPN hopping, which tends to make IP reputation worse; dropped.

### D2. Player: `BotCheck` stops instead of advancing
In `Player::fail_current`, a `BotCheck` error reports the message without the track prefix and calls `stop()`, keeping the current queue item. `Status::Stopped` with a current item already makes "Play" start that item again (`play()` → `start_current`), so retrying needs no new code path. `Unavailable` and other errors keep today's skip behavior. The rejected-stream retry (403 → re-resolve once) is unaffected because it happens before `fail_current`.

### D3. Downloads: first `BotCheck` stops everything
When a job finishes with `BotCheck`, the manager (under its entries lock):
1. marks that job `Failed(message)`;
2. for every other active job (queued or downloading): resets its run token, aborts its task, schedules temp cleanup (same path as `cancel`), and marks it `Failed(message)` – failed rather than cancelled, so "Retry" is offered and the reason is visible;
3. publishes a notice `(seq, message)` in `DownloadsState`.

Aborted attempts cannot report later (run token), so concurrent jobs that hit the same block do not add more notices. Jobs started after the stop run normally (no cooldown, by decision).

### D4. UI: downloads notice as a single toast
`KoiMusicApp` remembers the last downloads notice `seq` and shows a toast when it changes, the same way it already handles the player's `last_error`. The Downloads view needs no change: failed jobs already show their message and a Retry button.

### D5. Manual verification without YouTube
A real bot check cannot be triggered on demand. For manual checks, point Settings → yt-dlp at a small fake script that answers `--version` and `-v` normally and fails every other call with yt-dlp's bot-check error on stderr. This exercises the real classify → player/downloads → UI path. The script lives in the test fixtures (`crates/koi-core/tests/fixtures/fake-yt-dlp-botcheck.sh`) and is also used by a regular (not ignored) unit test of `YtDlp`.

## Risks / Trade-offs

- [YouTube changes the bot-check wording] → Detection already matches two phrasings; an unmatched wording falls back to today's behavior (`Unavailable`, skip). The fake-script test pins the current phrasing.
- [Stopping all downloads discards progress of running jobs] → Accepted by decision; files are re-downloaded on Retry (tracks are a few MB).
- [User presses play repeatedly during a block] → Each press is one request, initiated by the user; no automatic retries.
- [Fake yt-dlp script is POSIX shell] → Linux-only test and manual check; on Windows the test is skipped (`cfg(unix)`), the logic under test is platform-independent.
