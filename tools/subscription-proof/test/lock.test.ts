// Cross-process lock ownership. The lock is held by a separate process so ownership is real, and
// the test controls every step, so no scheduling luck is involved.
import { describe, expect, test } from "bun:test";
import { existsSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { withFileLock, withFileLockSync } from "../src/lock";

const lockModule = join(import.meta.dir, "..", "src", "lock.ts");

/** Starts a process that takes the lock, prints HELD, and keeps it until killed. */
async function holder(lockPath: string) {
  const script = `import { withFileLock } from ${JSON.stringify(lockModule)};
    await withFileLock(${JSON.stringify(lockPath)}, async () => { console.log("HELD"); await new Promise(() => {}); });`;
  const proc = Bun.spawn(["bun", "-e", script], { stdout: "pipe", stderr: "ignore" });
  const reader = proc.stdout.getReader();
  let out = "";
  while (!out.includes("HELD")) {
    const { value, done } = await reader.read();
    if (done) throw new Error("holder exited before taking the lock");
    out += new TextDecoder().decode(value);
  }
  return proc;
}

/** A pid that no process owns: a short-lived child after it has exited. */
const deadPid = () => Bun.spawnSync(["true"]).pid;

describe("cross-process lock", () => {
  test("a live owner keeps the lock even when the lock file names a dead process", async () => {
    // The reviewer's interleaving: a waiter that read a dead owner's pid must not remove or
    // bypass the lock a live process now holds.
    const path = join(mkdtempSync(join(tmpdir(), "nzube-lock-")), "x.lock");
    const live = await holder(path);
    try {
      writeFileSync(path, String(deadPid()));
      let entered = false;
      expect(() => withFileLockSync(path, () => (entered = true), { timeoutMs: 300 })).toThrow("lock timeout");
      expect(entered).toBe(false);
      expect(existsSync(path)).toBe(true);
    } finally {
      live.kill("SIGKILL");
      await live.exited;
    }
  });

  test("a crashed owner's lock is released without any takeover step", async () => {
    const path = join(mkdtempSync(join(tmpdir(), "nzube-lock-")), "x.lock");
    const crashed = await holder(path);
    crashed.kill("SIGKILL");
    await crashed.exited;
    expect(await withFileLock(path, async () => "entered", { timeoutMs: 2_000 })).toBe("entered");
  });

  test("two callers in one process exclude each other", async () => {
    const path = join(mkdtempSync(join(tmpdir(), "nzube-lock-")), "x.lock");
    let inside = 0;
    let maxInside = 0;
    const section = async () => {
      inside += 1;
      maxInside = Math.max(maxInside, inside);
      await Bun.sleep(50);
      inside -= 1;
    };
    await Promise.all([withFileLock(path, section), withFileLock(path, section), withFileLock(path, section)]);
    expect(maxInside).toBe(1);
  });
});
