# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed
- Clarified `Run.ok` docstring to explicitly state it is `False` for live runs since they have not completed.

## [0.1.0]

First release: a thin, faithful wrapper around the `tormoni` CLI.

### Added

- `Tormoni` (aliased `Sandbox`) with `run`, `dry_run`, `run_with`, `show` and
  `runs`. The binary is resolved from an explicit path, then `$TORMONI_CLI`,
  then `tormoni` on `PATH`.
- `RunOptions`, one field per CLI flag, validated and frozen at construction.
  Repeatable options accept a mapping, `(key, value)` pairs, or `"KEY=VALUE"`
  strings, from any iterable.
- `Run`, `Posture` and `File` records mirroring the `--json` document, with
  `Run.ok` meaning `end_kind == "exit" and end_code == 0`.
- `TormoniNotFound` and `TormoniError`, both under `TormoniException`.
  `TormoniError` carries Tormoni's own `stderr` unchanged plus `exit_code`.
- Type annotations throughout, with `py.typed` shipped in the wheel.

### Notes

- A guest command exiting non-zero is **not** an exception: it is a `Run` whose
  `ok` is false. Only a failure of Tormoni itself raises.
- No runtime dependencies, no retries, and no caching.
