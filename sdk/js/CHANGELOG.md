# Changelog

All notable changes to this project are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0]

First release.

### Added

- `Boxdesk` client wrapping the `boxdesk` CLI: `run`, `runSync`, `dryRun`, `show` and `runs`.
- `Run`, `Posture` and `RunFile` record types, mapped by hand from the CLI's JSON document.
- `BoxdeskException`, `BoxdeskNotFoundError` and `BoxdeskError`, the last carrying Boxdesk's
  own `stderr` verbatim alongside the process exit code.
- Binary resolution from an explicit path, then `$BOXDESK_CLI`, then `boxdesk` on `PATH`.
- ESM and CommonJS builds with type declarations for both.

[Unreleased]: https://github.com/kendricklawton/boxdesk-js/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/kendricklawton/boxdesk-js/releases/tag/v0.1.0
