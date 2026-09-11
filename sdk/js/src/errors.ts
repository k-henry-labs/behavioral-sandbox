/** Base class for every error this SDK raises. */
export class TormoniException extends Error {
  constructor(message: string) {
    super(message);
    this.name = "TormoniException";
  }
}

/** The `tormoni` binary could not be found or executed. */
export class TormoniNotFoundError extends TormoniException {
  /** The path that was tried. */
  readonly path: string;

  constructor(path: string) {
    super(
      `tormoni binary not found: ${path}. Install the Tormoni CLI and make sure ` +
        `it is on PATH, or set the TORMONI_CLI environment variable (or the ` +
        `client's "path" option) to its location.`,
    );
    this.name = "TormoniNotFoundError";
    this.path = path;
  }
}

/**
 * Tormoni itself failed: its stdout was not one JSON document, so no sandbox
 * record came back.
 *
 * A guest command exiting non-zero is *not* this — that is an ordinary
 * {@link Run} with a non-zero `endCode`.
 */
export class TormoniError extends TormoniException {
  /** Tormoni's stderr, verbatim. It is written for a person; show it unchanged. */
  readonly stderr: string;
  /** The process exit status, or `null` if it was killed by a signal. */
  readonly exitCode: number | null;

  constructor(stderr: string, exitCode: number | null, fallbackReason?: string) {
    super(stderr.trim() || fallbackReason || `tormoni failed with exit code ${exitCode}`);
    this.name = "TormoniError";
    this.stderr = stderr;
    this.exitCode = exitCode;
  }
}
