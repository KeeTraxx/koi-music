# Spec Delta

## MODIFIED Requirements

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
