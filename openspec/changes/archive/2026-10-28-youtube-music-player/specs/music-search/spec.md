# Spec Delta

## Purpose

Lets users find music on YouTube Music by text query and browse the tracks of albums, artists, and playlists returned by a search.

## ADDED Requirements

### Requirement: Text search across YouTube Music
The system SHALL accept a free-text query and return YouTube Music results grouped into songs, albums, artists, and playlists. Each result SHALL show a title, a subtitle (artist/owner), a thumbnail, and, for songs, a duration.

#### Scenario: Successful search
- **WHEN** the user submits the query "daft punk around the world"
- **THEN** the system shows song results that include the track title, artist name, duration, and thumbnail
- **AND** shows album, artist, and playlist results in their own groups when any exist

#### Scenario: Empty query
- **WHEN** the user submits a query that is empty or only whitespace
- **THEN** the system does not perform a search and keeps the current results unchanged

#### Scenario: No results
- **WHEN** a search returns no items
- **THEN** the system shows a "no results" message instead of an empty list

### Requirement: Filter search by result type
The system SHALL let the user restrict a search to a single result type (songs, albums, artists, or playlists).

#### Scenario: Songs-only filter
- **WHEN** the user selects the "Songs" filter and submits a query
- **THEN** only song results are shown

### Requirement: Paginated results
The system SHALL load additional results for a filtered search on demand when more results are available.

#### Scenario: Load more
- **WHEN** the user scrolls to the end of a filtered result list and more results exist
- **THEN** the next page is appended without duplicating existing entries

### Requirement: Search suggestions
The system SHALL show query suggestions while the user types, after a short debounce.

#### Scenario: Suggestions while typing
- **WHEN** the user types "radioh" and pauses
- **THEN** the system shows suggestions such as "radiohead"
- **AND** selecting a suggestion submits it as the query

### Requirement: Browse album, playlist, and artist results
The system SHALL let the user open an album or playlist result to view its full track list, and open an artist result to view that artist's top songs and albums.

#### Scenario: Open album
- **WHEN** the user opens an album result
- **THEN** the system shows the album title, artist, cover, and ordered track list
- **AND** each track can be played, queued, added to a playlist, or downloaded

#### Scenario: Open artist
- **WHEN** the user opens an artist result
- **THEN** the system shows the artist's top songs and albums

### Requirement: Search failure is reported
The system SHALL show an error message with a retry action when a search or browse request fails, and SHALL NOT crash or clear the existing view.

#### Scenario: Network unavailable
- **WHEN** the user searches while offline
- **THEN** the system shows an error explaining the request failed and offers "Retry"
