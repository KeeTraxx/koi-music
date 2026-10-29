# Spec Delta

## ADDED Requirements

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
