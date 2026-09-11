import { describe, expect, test } from "vitest";
import { Tormoni } from "../src/client";
import type { Run } from "../src/types";

// The two rules the whole contract rests on, and the ones most likely to be broken by a
// well-meaning change: an environment value goes out to the guest but never comes back in a
// record, and a guest command exiting non-zero is a record and not an error.
//
// These run against the REAL core, not a stub. There is no binary to stand in for any more: the
// SDK calls `execute_sandbox` in this process, so a fake would be testing itself. `dryRun`
// settles a posture without booting, which is what lets the contract be checked on a machine with
// no hypervisor.

const tormoni = new Tormoni();

describe("the environment asymmetry", () => {
  const SECRET = "s3cret-value-nobody-should-see";

  test("a value goes out whole and only the name comes back", async () => {
    const run = await tormoni.dryRun(["env"], { env: { API_KEY: SECRET, DEBUG: "1" } });
    expect(run.posture.env).toEqual(["API_KEY", "DEBUG"]);
  });

  test("no corner of the record holds the value", async () => {
    const run = await tormoni.dryRun(["env"], { env: { API_KEY: SECRET } });
    // Serialising the whole object is the point: a field added later that carried the value
    // would slip past an assertion naming only the fields that exist today.
    expect(JSON.stringify(run)).not.toContain(SECRET);
  });

  test("a value containing '=' survives the split", async () => {
    // `splitPair` once split on every `=`, which truncated a base64 value at its padding.
    const run = await tormoni.dryRun(["env"], { env: { TOKEN: "a=b=c" } });
    expect(run.posture.env).toEqual(["TOKEN"]);
  });
});

describe("the posture is the one that was asked for", () => {
  test("limits, network and rootfs come back as set", async () => {
    const run = await tormoni.dryRun(["true"], {
      vcpus: 2,
      mem: 1024,
      net: "tsi",
      rootfs: "writable",
    });
    expect(run.posture.vcpus).toBe(2);
    expect(run.posture.memMib).toBe(1024);
    expect(run.posture.network).toBe("tsi");
    expect(run.posture.rootfs).toBe("writable");
  });

  test("the defaults are the CLI's own, not this SDK's", async () => {
    const run = await tormoni.dryRun(["true"]);
    expect(run.posture.vcpus).toBe(1);
    expect(run.posture.memMib).toBe(512);
    expect(run.posture.network).toBe("none");
    expect(run.posture.rootfs).toBe("read-only");
    expect(run.posture.results).toBe(true);
  });

  test("mounts and shares keep their order and their pairing", async () => {
    const run = await tormoni.dryRun(["true"], {
      mounts: [["/mnt/a", "/tmp"], ["/mnt/b", "/var/tmp"]],
      shares: [["tag", "/tmp"]],
    });
    expect(run.posture.mounts).toEqual([["/mnt/a", "/tmp"], ["/mnt/b", "/var/tmp"]]);
    expect(run.posture.shares).toEqual([["tag", "/tmp"]]);
  });
});

describe("the boundary refuses what it cannot represent", () => {
  // `NonZeroU8` and `NonZeroU32` are the core's types. Zero has to be refused HERE, with a
  // sentence, rather than silently becoming the default on the way through.
  test("zero vcpus is refused, not rounded up", async () => {
    await expect(tormoni.dryRun(["true"], { vcpus: 0 })).rejects.toThrow(/at least 1|whole number/);
  });

  test("zero memory is refused", async () => {
    await expect(tormoni.dryRun(["true"], { mem: 0 })).rejects.toThrow(/at least 1/);
  });

  test("more vcpus than a u8 holds is refused", async () => {
    await expect(tormoni.dryRun(["true"], { vcpus: 256 })).rejects.toThrow(/1 to 255/);
  });

  test("an unknown posture word is refused rather than read as the default", async () => {
    // The dangerous version of this bug is silent: "writeable" falling through to read-only
    // gives a caller a sandbox they did not ask for and no sign of it.
    await expect(tormoni.dryRun(["true"], { rootfs: "writeable" as never })).rejects.toThrow(
      /read-only/,
    );
    await expect(tormoni.dryRun(["true"], { net: "host" as never })).rejects.toThrow(/none/);
  });

  test("an empty command is refused", async () => {
    await expect(tormoni.dryRun([])).rejects.toThrow(/empty/);
  });
});

describe("a dry run has settled a posture and nothing more", () => {
  test("it carries no end and no directory", async () => {
    const run: Run = await tormoni.dryRun(["echo", "hi"]);
    expect(run.command).toEqual(["echo", "hi"]);
    expect(run.verb).toBe("run");
    expect(run.endKind).toBeUndefined();
    expect(run.endedMs).toBeUndefined();
    expect(run.dir).toBeUndefined();
    expect(run.ok).toBe(false);
  });

  test("times are numbers, not BigInts", async () => {
    // napi maps `u64` to BigInt, which is contagious: `run.startedMs - t0` throws on a mix. The
    // Rust side uses i64 for exactly this reason, and this is what holds it there.
    const run = await tormoni.dryRun(["true"]);
    expect(typeof run.startedMs).toBe("number");
    expect(run.startedMs).toBeGreaterThan(0);
  });
});

describe("a missing run is a refusal a caller can read", () => {
  test("show names the id it could not find", async () => {
    await expect(tormoni.show("1-definitely-not-a-run")).rejects.toThrow(/no run/);
  });
});
