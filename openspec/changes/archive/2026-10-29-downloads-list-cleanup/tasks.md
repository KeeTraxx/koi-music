# Tasks

## 1. Downloads manager

- [x] 1.1 Remove `JobState::Completed` and `Manager::clear_finished`; drop a job from the list when its attempt succeeds (after the library records the file). Verify: `cargo test -p koi-core downloads` passes with tests updated to wait for the job to leave the list.
- [x] 1.2 Add a test where one download fails and another completes. Verify: `finished_downloads_leave_the_list_failed_ones_stay` passes. Only the failed job remains, and the completed track is recorded as downloaded.

## 2. Downloads view

- [x] 2.1 Replace "Clear finished" with "📂 Open Directory", which creates the folder if needed and starts `xdg-open` (Linux) or `explorer` (Windows) without blocking the UI. Errors are shown as a toast. Verify: in the running app, clicking it before any download creates the folder and calls the file manager with its path.
- [x] 2.2 Remove the "✔ Completed" job row. Verify: after downloading a track, the Queue section is gone and the track is listed under "Downloaded tracks".

## 3. Integration checks

- [x] 3.1 Run `cargo clippy --workspace --all-targets --locked -- -D warnings` and `cargo test --workspace --locked`. Verify: both succeed.
