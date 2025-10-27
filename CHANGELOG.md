# Changelog

## [0.6.0] - 2025-10-27

### Added
- Resume the latest Codex session automatically when launching the agent, making `xlaude open` pick up the previous conversation without manual steps. (#70)

### Fixed
- Prevent duplicate worktree registration when adding existing instances. (#64)
- Eliminate duplicate input characters in dashboard create mode. (#66)

### Documentation
- Document how to configure and use Codex with xlaude. (#72)

### Maintenance
- Bump the CLI version to 0.6.0 in preparation for release.
- Refresh dependencies to the latest compatible releases, including `clap` 4.5.50, `clap_complete` 4.5.59, `serde` 1.0.228, `serde_json` 1.0.145, `dialoguer` 0.12.0, `directories` 6.0.0, `chrono` 0.4.42, `rand` 0.9.2, `bip39` 2.2.0, `anyhow` 1.0.100, `ratatui` 0.29.0, `crossterm` 0.29.0, `atty` 0.2.14, `insta` 1.43.2, `tempfile` 3.23.0, `assert_cmd` 2.0.17, and `regex` 1.12.2.
