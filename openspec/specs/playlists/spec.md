# playlists Specification

## Purpose

Lets users build and manage their own local playlists of YouTube Music tracks, persisted on the device, and import remote albums or playlists as local ones.

## Requirements

### Requirement: Create, rename, and delete playlists
The system SHALL let the user create a playlist with a non-empty name, rename it, and delete it. Deleting SHALL require confirmation and SHALL NOT delete downloaded audio files.

#### Scenario: Create playlist
- **WHEN** the user creates a playlist named "Focus"
- **THEN** an empty playlist "Focus" appears in the playlist list

#### Scenario: Reject empty name
- **WHEN** the user tries to create or rename a playlist with an empty or whitespace-only name
- **THEN** the system rejects it and shows a validation message

#### Scenario: Delete keeps downloads
- **WHEN** the user confirms deletion of a playlist whose tracks were downloaded
- **THEN** the playlist is removed and the downloaded files remain on disk

### Requirement: Manage playlist tracks
The system SHALL let the user add any track (from search, album/artist views, queue, or another playlist) to a playlist, remove tracks, and reorder tracks. Adding a track already in the playlist SHALL require user confirmation.

#### Scenario: Add track
- **WHEN** the user chooses "Add to playlist → Focus" on a song
- **THEN** the song is appended to "Focus"

#### Scenario: Duplicate warning
- **WHEN** the user adds a song that is already in "Focus"
- **THEN** the system asks for confirmation before adding it again

#### Scenario: Reorder
- **WHEN** the user drags the third track of a playlist to the first position
- **THEN** the playlist order is updated and persisted

### Requirement: Playlists persist locally
The system SHALL store playlists and their track metadata (id, title, artists, album, duration, thumbnail) on the local device so they survive restarts and can be listed without network access.

#### Scenario: Survives restart
- **WHEN** the user adds tracks to a playlist and restarts the app
- **THEN** the playlist and its tracks in the same order are present

#### Scenario: Offline listing
- **WHEN** the app starts without network access
- **THEN** all playlists and their track lists are viewable

### Requirement: Play a playlist
The system SHALL let the user play a playlist from the start, play it shuffled, or start from a chosen track, replacing the current queue.

#### Scenario: Start from chosen track
- **WHEN** the user selects the fifth track in a playlist
- **THEN** the queue becomes the playlist's tracks and playback starts at the fifth track

### Requirement: Import remote album or playlist
The system SHALL let the user save a YouTube Music album or playlist as a new local playlist containing its tracks in order. The imported playlist is a snapshot and SHALL NOT change when the remote one changes.

#### Scenario: Save album as playlist
- **WHEN** the user chooses "Save as playlist" on an album view
- **THEN** a new local playlist named after the album is created with all album tracks in order

### Requirement: Recently played history
The system SHALL record tracks the user has played and show them in a "Recently played" list, most recent first, limited to the last 200 entries.

#### Scenario: History entry recorded
- **WHEN** a track has been playing for at least 30 seconds
- **THEN** it appears at the top of "Recently played"
