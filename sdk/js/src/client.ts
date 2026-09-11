import { entries } from "./args";
import type { Run, RunOptions } from "./types";

// napi's generated loader, which picks the `.node` for this platform at run time.
//
// It is EXTERNAL to the bundle (see `tsup.config.ts`) and must stay that way: it contains a
// `require` for every platform triple napi knows, and esbuild following those tries to resolve
// `boxdesk-js.android-arm64.node` and fails the whole build. Kept external, the import survives
// into both outputs and resolves to `index.js` beside the package at run time.
import * as core from "../index.js";

export interface BoxdeskConfig {
  /**
   * Unused, and kept so a `new Boxdesk({ path })` written against the old subprocess SDK still
   * compiles. There is no binary to find: the sandbox runs through the linked core.
   *
   * @deprecated The SDK no longer spawns the `boxdesk` CLI.
   */
  path?: string;
}

/**
 * A handle on the Boxdesk core.
 *
 * Nothing here spawns a process or speaks HTTP. `run` calls the same `execute_sandbox` the
 * `boxdesk` binary calls, so a run started here leaves the record a run started at a keyboard
 * leaves.
 */
export class Boxdesk {
  constructor(_config: BoxdeskConfig = {}) {}

  /**
   * Boot a fresh sandbox, run one command, wait, and return the record.
   *
   * A command that exits non-zero is **not** an error: it comes back as a {@link Run} whose `ok`
   * is false. Only a failure of Boxdesk itself throws.
   */
  async run(command: readonly string[], options: RunOptions = {}): Promise<Run> {
    return this.runSync(command, options);
  }

  /** {@link run}, without the promise. Booting a sandbox blocks either way. */
  runSync(command: readonly string[], options: RunOptions = {}): Run {
    return this.invoke(command, options, false);
  }

  /** Settle and return the posture without booting anything. The record has no end. */
  async dryRun(command: readonly string[], options: RunOptions = {}): Promise<Run> {
    return this.invoke(command, options, true);
  }

  /** One filed run, by id or by name. */
  async show(id: string): Promise<Run> {
    return core.show(id);
  }

  /** The filed runs, newest first. Without `all`, only the ones still running. */
  async runs(all = false): Promise<Run[]> {
    return core.runs(all);
  }

  private invoke(command: readonly string[], options: RunOptions, dryRun: boolean): Run {
    const pairs = (value: unknown, name: string): Array<[string, string]> =>
      value ? Array.from(entries(value, name)).map(split) : [];
    return core.runSandbox(
      options.name,
      Array.from(command),
      options.root,
      options.vcpus,
      options.mem,
      options.workdir,
      pairs(options.mounts, "mounts").map(([a, b]) => [a, b]),
      pairs(options.shares, "shares").map(([a, b]) => [a, b]),
      options.net,
      options.rootfs,
      // The WHOLE `KEY=VALUE` entry. An earlier build sent only the name here, so the guest was
      // handed a variable set to nothing; the record still keeps names alone, but it is the core
      // that makes that cut, not this file.
      options.env ? Array.from(entries(options.env, "env")) : [],
      options.noResults ?? false,
      options.keep ?? false,
      dryRun,
      options.gpu ?? false,
      options.sound ?? false,
    );
  }
}

/** `KEY=VALUE` as its two halves, splitting at the FIRST `=` so a value may contain one. */
function split(entry: string): [string, string] {
  const at = entry.indexOf("=");
  return at === -1 ? [entry, ""] : [entry.slice(0, at), entry.slice(at + 1)];
}
