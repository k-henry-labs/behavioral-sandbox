/** Base class for every error this SDK raises. */
export class BoxdeskException extends Error {
  constructor(message: string) {
    super(message);
    this.name = "BoxdeskException";
  }
}

/** The `boxdesk` binary could not be found or executed. */
export class BoxdeskNotFoundError extends BoxdeskException {
  /** The path that was tried. */
  readonly path: string;

  constructor(path: string) {
    super(
      `boxdesk binary not found: ${path}. Install the Boxdesk CLI and make sure ` +
        `it is on PATH, or set the BOXDESK_CLI environment variable (or the ` +
        `client's "path" option) to its location.`,
    );
    this.name = "BoxdeskNotFoundError";
    this.path = path;
  }
}

/**
 * Boxdesk itself failed: its stdout was not one JSON document, so no sandbox
 * record came back.
 *
 * A guest command exiting non-zero is *not* this — that is an ordinary
 * {@link Run} with a non-zero `endCode`.
 */
export class BoxdeskError extends BoxdeskException {
  /** Boxdesk's stderr, verbatim. It is written for a person; show it unchanged. */
  readonly stderr: string;
  /** The process exit status, or `null` if it was killed by a signal. */
  readonly exitCode: number | null;

  constructor(stderr: string, exitCode: number | null, fallbackReason?: string) {
    super(stderr.trim() || fallbackReason || `boxdesk failed with exit code ${exitCode}`);
    this.name = "BoxdeskError";
    this.stderr = stderr;
    this.exitCode = exitCode;
  }
}
