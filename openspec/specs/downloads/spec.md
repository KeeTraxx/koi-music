# downloads Specification

## Purpose

Lets users save YouTube Music tracks to disk as tagged audio files for offline listening, with visible progress and recoverable failures.

## Requirements

### Requirement: Download a track as an audio file
The system SHALL download the best available audio-only stream of a track and save it as an audio file in the configured download folder, named `<Artist> - <Title>.<ext>` with filesystem-unsafe characters replaced. File names SHALL also be valid on Windows, including avoiding names Windows reserves for devices.

#### Scenario: Successful download
- **WHEN** the user chooses "Download" on a song
- **THEN** an audio file for that song appears in the download folder
- **AND** the song is marked as downloaded wherever it is shown

#### Scenario: Filename collision
- **WHEN** a file with the target name already exists and belongs to a different track
- **THEN** the system saves the new file with a numeric suffix instead of overwriting

#### Scenario: Windows-reserved file name
- **WHEN** the artist and title would produce a file name that Windows reserves (for example `CON` or `NUL`, with or without extension)
- **THEN** the system saves the file under a changed name that Windows accepts

### Requirement: Embedded metadata
Downloaded files SHALL contain embedded title, artist, album, and cover art when that information is available.

#### Scenario: Tags present
- **WHEN** a downloaded song is opened in another music player
- **THEN** it shows the correct title, artist, album, and cover image

### Requirement: Download a collection
The system SHALL let the user download all tracks of a local playlist, album, or remote playlist in one action, skipping tracks already downloaded.

#### Scenario: Download playlist
- **WHEN** the user chooses "Download all" on a 12-track playlist where 2 tracks are already downloaded
- **THEN** the system queues 10 downloads

### Requirement: Download queue and progress
The system SHALL run downloads in a queue with a limited number of concurrent downloads (default 3) and SHALL show each download's state (queued, downloading with percentage, completed, failed).

#### Scenario: Progress shown
- **WHEN** a download is in progress
- **THEN** the downloads view shows its percentage, updating at least once per second

### Requirement: Cancel and retry
The system SHALL let the user cancel a queued or running download and retry a failed one. Cancelled or failed downloads SHALL NOT leave partial files in the download folder.

#### Scenario: Cancel running download
- **WHEN** the user cancels a download at 50%
- **THEN** the download stops and no partial file remains in the download folder

#### Scenario: Retry failed download
- **WHEN** a download fails due to a network error and the user presses "Retry"
- **THEN** the download restarts and can complete successfully

### Requirement: Manage downloaded tracks
The system SHALL list all downloaded tracks in a "Downloads" library view and let the user delete a download, which removes the file from disk and clears the downloaded mark. The download folder SHALL be configurable.

#### Scenario: Delete download
- **WHEN** the user deletes a downloaded track
- **THEN** its file is removed from disk and the track remains in any playlists, now streaming instead

#### Scenario: Missing file detected
- **WHEN** a downloaded file was removed outside the app
- **THEN** the system clears its downloaded mark and streams the track on next play
