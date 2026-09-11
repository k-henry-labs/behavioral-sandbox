# Changelog

## v0.1.0

First release.

- `Client.Run`, `Client.DryRun`, `Client.Show` and `Client.Runs`, each taking a `context.Context`
  and honouring cancellation.
- `Run`, `Posture`, `Mount`, `Share` and `File`, with explicit JSON tags for every field, plus
  `Run.OK`.
- `RunOptions`, mapping one-to-one onto the flags `tormoni run` accepts.
- `ErrNotFound` and `*Error`, which carries Tormoni's stderr verbatim, its exit code, and the
  argument list with every `--env` value redacted.
- Waiting on the subprocess is bounded, so a process the CLI leaves holding its stdout cannot hang
  a call indefinitely.

Long-lived sandboxes (`tormoni up`) are not covered.
