# Spec Delta

## ADDED Requirements

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
