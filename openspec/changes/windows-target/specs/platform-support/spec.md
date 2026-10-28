# Spec Delta

## Purpose

Defines which operating systems koi-music supports, how it is built and distributed for each, and the platform conventions it follows.

## ADDED Requirements

### Requirement: Supported platforms
The system SHALL support Linux (x86_64) and Windows 10/11 (x86_64). Every change to the main branch SHALL be built and its automated tests run on both platforms.

#### Scenario: Windows build checked on every push
- **WHEN** a commit is pushed to the repository
- **THEN** an automated build compiles the app and runs the test suite on Linux and on Windows
- **AND** a failure on either platform marks the commit as failing

### Requirement: Prebuilt Windows binary
The system SHALL publish a prebuilt Windows executable for every released version, so Windows users do not need a Rust toolchain. Releases SHALL be downloadable by anyone, without a GitHub account. The binary is not code-signed and no installer is provided.

#### Scenario: Version tag produces a Windows download
- **WHEN** a version tag such as `v0.2.0` is pushed
- **THEN** a GitHub release for that version contains a zip archive with `koi-music.exe`

#### Scenario: Anonymous download
- **WHEN** a user who is not logged in to GitHub opens the release page
- **THEN** they can download the Windows zip archive

### Requirement: Windows application conventions
On Windows the app SHALL behave like a normal desktop application: no console window, data stored under the user's application-data folder, and downloads saved to the user's Music folder by default.

#### Scenario: No console window
- **WHEN** the user starts `koi-music.exe` from Explorer
- **THEN** only the app window opens, without an accompanying console window

#### Scenario: Default locations
- **WHEN** the app runs on Windows for the first time
- **THEN** its library is stored under `%APPDATA%`
- **AND** the default download folder is `koi-music` inside the user's Music folder

### Requirement: User-installed tools on Windows
On Windows the app SHALL find yt-dlp (`yt-dlp.exe`) on `PATH` or at the path configured in Settings, and SHALL show the same notices as on Linux when yt-dlp or a JavaScript runtime is missing. The app SHALL NOT bundle or download these tools.

#### Scenario: yt-dlp installed with winget
- **WHEN** yt-dlp and Deno were installed with `winget` and the app is started
- **THEN** Settings shows the detected yt-dlp version and JavaScript runtime
- **AND** non-downloaded tracks can be streamed

#### Scenario: yt-dlp missing on Windows
- **WHEN** yt-dlp is not installed
- **THEN** the app shows that streaming and downloads are disabled and how to install yt-dlp
