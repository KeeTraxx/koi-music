# Tasks

## 1. Bot-check message

- [x] 1.1 Change the bot-check message and stop appending the update hint to it (still appended to `Unavailable`), and show bot-check errors without the "YouTube bot check:" prefix (design D1); verify updated `classify_failure` and `Error` display tests
- [x] 1.2 Add the fake yt-dlp fixture `crates/koi-core/tests/fixtures/fake-yt-dlp-botcheck.sh` (answers `--version`/`-v`, fails everything else with the bot-check error) and a `cfg(unix)` test that `YtDlp::resolve` and `YtDlp::download` with it return `ErrorKind::BotCheck` with the new message (design D5); verify the test passes and `probe()` still succeeds with the fixture

## 2. Playback stops on a bot check

- [x] 2.1 Stop playback on a `BotCheck` stream error instead of advancing, keep the current item, report the message once (design D2); verify player tests with a `FakeSource` that can simulate a bot check: a 10-track queue makes exactly one stream request and stops with one error, `Play` retries the same track, a downloaded track still plays, and region-blocked tracks still skip

## 3. Downloads stop on a bot check

- [x] 3.1 On the first `BotCheck` failure, stop all active downloads (abort, temp cleanup, mark failed with the message) and publish one notice in `DownloadsState` (design D3); verify manager tests with a fake backend: 3 running + queued jobs all end `Failed` with the message, no further fetch calls start, no files remain in the download folder, exactly one notice, and Retry afterwards works
- [x] 3.2 Show the downloads notice as one toast in the app (design D4); verify manually with the fake yt-dlp fixture set as the yt-dlp path in Settings: "Download all" on an album shows one toast and all jobs failed with Retry, and playing a non-downloaded track shows one message and stops

## 4. Documentation and release

- [x] 4.1 Describe the bot-check behavior (playback stops, downloads stop, try again later, no automatic resume) in the README next to the existing bot-check note; verify it matches the delta specs
- [x] 4.2 Run `cargo test --workspace` and `cargo clippy --workspace --all-targets -- -D warnings`, push, and confirm CI is green on Linux and Windows
- [ ] 4.3 Bump the workspace version to 0.1.1, push tag `v0.1.1`, and verify `gh release view v0.1.1` lists `koi-music-0.1.1-windows-x86_64.zip` and that it downloads without authentication

## Workflow follow-up

- Archive the change with `/opsx:archive` after review.
