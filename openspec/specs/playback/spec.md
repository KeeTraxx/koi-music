# playback Specification

## Purpose

Plays YouTube Music tracks as audio-only streams (or from local downloaded files) through a play queue with standard transport controls.

## Requirements

### Requirement: Play a track
The system SHALL play the audio of a selected track without video. Playback SHALL start before the whole track has been fetched.

#### Scenario: Play from search results
- **WHEN** the user selects "Play" on a song result
- **THEN** audio starts playing and the now-playing bar shows the track title, artist, thumbnail, elapsed time, and duration

#### Scenario: Stream unavailable
- **WHEN** a track has no playable audio stream (e.g. region-blocked or removed)
- **THEN** the system shows an error naming the track and advances to the next queued track if one exists

### Requirement: Prefer downloaded copy
The system SHALL play a track from its downloaded file when one exists, instead of streaming it.

#### Scenario: Offline playback of downloaded track
- **WHEN** the user plays a track that has been downloaded while the network is unavailable
- **THEN** the track plays from the local file

### Requirement: Transport controls
The system SHALL provide play/pause, next, previous, and seek controls for the current track.

#### Scenario: Pause and resume
- **WHEN** the user presses pause during playback and then presses play
- **THEN** playback stops and later resumes from the same position

#### Scenario: Seek
- **WHEN** the user drags the progress bar to 1:30 on a 4:00 track
- **THEN** playback continues from 1:30

#### Scenario: Previous restarts current track
- **WHEN** the user presses previous more than 3 seconds into a track
- **THEN** the current track restarts from 0:00
- **AND** pressing previous within the first 3 seconds plays the preceding queue item

### Requirement: Volume control
The system SHALL allow adjusting and muting the output volume, and SHALL remember the volume level across restarts.

#### Scenario: Volume persists
- **WHEN** the user sets volume to 40% and restarts the app
- **THEN** the volume is 40% after restart

### Requirement: Play queue
The system SHALL maintain an ordered play queue. Users SHALL be able to play a list (replacing the queue), add a track to the end, insert it to play next, remove items, reorder items, and clear the queue.

#### Scenario: Play next
- **WHEN** the user chooses "Play next" on a track while another track is playing
- **THEN** that track plays immediately after the current one finishes

#### Scenario: Play an album
- **WHEN** the user plays an album
- **THEN** the queue is replaced with the album's tracks in order and the first track starts

#### Scenario: End of queue
- **WHEN** the last track in the queue finishes and repeat is off
- **THEN** playback stops and the now-playing bar keeps showing the last track

### Requirement: Shuffle and repeat
The system SHALL support shuffle (randomized order of the remaining queue, reversible) and repeat modes off, repeat-all, and repeat-one.

#### Scenario: Repeat one
- **WHEN** repeat-one is enabled and the current track ends
- **THEN** the same track plays again from the start

#### Scenario: Repeat all
- **WHEN** repeat-all is enabled and the last queue item ends
- **THEN** playback continues with the first queue item

#### Scenario: Unshuffle restores order
- **WHEN** the user enables and then disables shuffle
- **THEN** the queue returns to its original order with the current track unchanged

### Requirement: Automatic advance
The system SHALL automatically start the next queue item when the current track finishes.

#### Scenario: Gapless advance not required
- **WHEN** a track finishes and another item is queued
- **THEN** the next item starts playing without user action

### Requirement: Media keys and OS integration
The system SHALL respond to OS media keys (play/pause, next, previous) and expose now-playing metadata to the OS media controls where the platform supports it.

#### Scenario: Media key pause
- **WHEN** the user presses the keyboard play/pause media key while the app is in the background
- **THEN** playback toggles between playing and paused

### Requirement: YouTube bot check stops playback
When YouTube refuses to provide a stream because it asks to confirm the user is not a bot, the system SHALL stop playback instead of advancing to the next queued track, and SHALL show a single message saying that YouTube is temporarily limiting requests from this network and to try again later. Tracks that are genuinely unavailable (removed, region-blocked) still advance as before.

#### Scenario: Bot check while playing a queue
- **WHEN** the next track of a 10-track queue cannot be streamed because YouTube asks to confirm the user is not a bot
- **THEN** playback stops on that track and the remaining tracks are not tried
- **AND** one message tells the user that YouTube is limiting requests and to try again later

#### Scenario: Trying again later
- **WHEN** playback stopped because of a bot check and the user presses play
- **THEN** the system tries to play the same track again

#### Scenario: Downloaded tracks are unaffected
- **WHEN** the stopped track or a later one is downloaded
- **THEN** pressing play plays it from the local file without contacting YouTube
