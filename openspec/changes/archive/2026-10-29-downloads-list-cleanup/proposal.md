# Proposal

## Why

The downloads list keeps every finished download until the user presses "Clear finished". The same tracks already appear under "Downloaded tracks" on the same page, so the list fills up with duplicates that need manual cleanup. Users also have no quick way to reach the downloaded files outside the app.

## What Changes

- A successful download leaves the downloads list on its own. The track then appears under "Downloaded tracks".
- Failed and cancelled downloads stay listed, because they still offer "Retry".
- **BREAKING (UI)**: Remove the "Clear finished" button. Remove the "completed" job state from the downloads manager.
- Add an "Open Directory" button in its place. It opens the download folder in the OS file manager, creating the folder first if nothing has been downloaded yet.

## Capabilities

### New Capabilities
<!-- none -->

### Modified Capabilities
- `downloads`: "Download queue and progress" no longer lists completed downloads. A new requirement covers opening the download folder.

## Impact

- `crates/koi-core/src/downloads/mod.rs`: `JobState::Completed` and `Manager::clear_finished` are removed. A successful job is dropped from the list once its track is in the library. Tests are updated.
- `crates/koi-music/src/views/downloads.rs`: the button is replaced and the "Completed" row is removed. Opening the folder runs `xdg-open` (Linux) or `explorer` (Windows), with no new dependency.
