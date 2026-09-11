# Changelog

All notable changes to this crate are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this crate adheres to
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0]

First release. Binds to the `tormoni` CLI's `--json` contract and nothing else.

### Added

- `Tormoni`, a client and builder in one. One builder method per `tormoni run` flag, no defaults
  of its own, reusable across calls.
- `run`, `dry_run`, `show` and `runs`, covering `tormoni run`, `tormoni run --dry-run`,
  `tormoni show` and `tormoni ls`.
- `Run`, carrying every field of a run document, plus `ok()` and an `End` reconstructed from the
  `end_kind` / `end_code` pair — the raw pair stays available.
- `Error` with `NotFound`, `Failed` and `Parse`. A guest command that exits non-zero is `Ok`, not
  an error; Tormoni's own stderr text reaches the caller unchanged.
- Binary resolution in order: `Tormoni::binary()`, then `$TORMONI_CLI`, then `tormoni` on `PATH`.

[Unreleased]: https://github.com/tormoni/tormoni-rust/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/tormoni/tormoni-rust/releases/tag/v0.1.0
