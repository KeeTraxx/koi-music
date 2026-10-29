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
The system SHALL run downloads in a queue with a limited number of concurrent downloads (default 3) and SHALL show the state of each download that is queued, running, failed or cancelled (queued, downloading with percentage, failed, cancelled). A download that completes SHALL leave this list without user action and appear among the downloaded tracks.

#### Scenario: Progress shown
- **WHEN** a download is in progress
- **THEN** the downloads view shows its percentage, updating at least once per second

#### Scenario: Completed download leaves the list
- **WHEN** a download completes while another download has failed
- **THEN** the completed track disappears from the downloads list and is listed among the downloaded tracks
- **AND** the failed download stays listed with its "Retry" button

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

### Requirement: YouTube bot check stops all downloads
When a download fails because YouTube asks to confirm the user is not a bot, the system SHALL stop all queued and running downloads, mark them as failed with a message saying that YouTube is temporarily limiting requests and to try again later, and show that message once. The system SHALL NOT resume them automatically.

#### Scenario: Bot check during "Download all"
- **WHEN** 3 downloads are running and 20 are queued and one of them hits YouTube's bot check
- **THEN** the running downloads stop, no queued download starts, and all of them show as failed with the "try again later" message
- **AND** no partial files remain in the download folder
- **AND** the user sees one notice, not one per download

#### Scenario: No automatic resume
- **WHEN** downloads were stopped by a bot check and the user does nothing
- **THEN** they stay failed until the user presses "Retry" or starts new downloads

### Requirement: Open the download folder
The downloads view SHALL offer an "Open Directory" action that opens the current download folder in the operating system's file manager. If the folder does not exist yet, the system SHALL create it first. If the file manager cannot be started, the system SHALL show an error message.

#### Scenario: Open before the first download
- **WHEN** nothing has been downloaded yet and the user presses "Open Directory"
- **THEN** the download folder is created and opened in the file manager

#### Scenario: Open the folder with downloads
- **WHEN** tracks have been downloaded and the user presses "Open Directory"
- **THEN** the file manager shows the folder containing the downloaded files
