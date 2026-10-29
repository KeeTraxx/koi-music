# Spec Delta

## MODIFIED Requirements

### Requirement: Download queue and progress
The system SHALL run downloads in a queue with a limited number of concurrent downloads (default 3) and SHALL show the state of each download that is queued, running, failed or cancelled (queued, downloading with percentage, failed, cancelled). A download that completes SHALL leave this list without user action and appear among the downloaded tracks.

#### Scenario: Progress shown
- **WHEN** a download is in progress
- **THEN** the downloads view shows its percentage, updating at least once per second

#### Scenario: Completed download leaves the list
- **WHEN** a download completes while another download has failed
- **THEN** the completed track disappears from the downloads list and is listed among the downloaded tracks
- **AND** the failed download stays listed with its "Retry" button

## ADDED Requirements

### Requirement: Open the download folder
The downloads view SHALL offer an "Open Directory" action that opens the current download folder in the operating system's file manager. If the folder does not exist yet, the system SHALL create it first. If the file manager cannot be started, the system SHALL show an error message.

#### Scenario: Open before the first download
- **WHEN** nothing has been downloaded yet and the user presses "Open Directory"
- **THEN** the download folder is created and opened in the file manager

#### Scenario: Open the folder with downloads
- **WHEN** tracks have been downloaded and the user presses "Open Directory"
- **THEN** the file manager shows the folder containing the downloaded files
