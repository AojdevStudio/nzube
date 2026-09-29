// Cross-process mutual exclusion with an exclusively created lock file holding the owner's pid.
// A lock whose owner process no longer exists is treated as stale and taken over.
import { closeSync, mkdirSync, openSync, readFileSync, rmSync, writeSync } from "node:fs";
import { dirname } from "node:path";
import { SafeError } from "./diagnostics";

const WAIT_MS = 25;
const TIMEOUT_MS = 30_000;

function tryAcquire(path: string): boolean {
  try {
    const fd = openSync(path, "wx", 0o600);
    writeSync(fd, String(process.pid));
    closeSync(fd);
    return true;
  } catch (e) {
    if ((e as NodeJS.ErrnoException).code !== "EEXIST") throw e;
    if (ownerGone(path)) rmSync(path, { force: true });
    return false;
  }
}

function ownerGone(path: string): boolean {
  try {
    const pid = Number(readFileSync(path, "utf8"));
    if (!Number.isInteger(pid) || pid <= 0) return false; // being written; retry
    process.kill(pid, 0);
    return false;
  } catch (e) {
    return (e as NodeJS.ErrnoException).code === "ESRCH";
  }
}

const release = (path: string) => rmSync(path, { force: true });

/** Runs `fn` while holding the lock at `path`, blocking the thread while waiting. For short critical sections. */
export function withFileLockSync<T>(path: string, fn: () => T): T {
  mkdirSync(dirname(path), { recursive: true });
  const deadline = Date.now() + TIMEOUT_MS;
  const sleeper = new Int32Array(new SharedArrayBuffer(4));
  while (!tryAcquire(path)) {
    if (Date.now() > deadline) throw new SafeError("storage_error", "lock timeout");
    Atomics.wait(sleeper, 0, 0, WAIT_MS);
  }
  try {
    return fn();
  } finally {
    release(path);
  }
}

/** Runs async `fn` while holding the lock at `path`, yielding while waiting. */
export async function withFileLock<T>(path: string, fn: () => Promise<T>): Promise<T> {
  mkdirSync(dirname(path), { recursive: true });
  const deadline = Date.now() + TIMEOUT_MS;
  while (!tryAcquire(path)) {
    if (Date.now() > deadline) throw new SafeError("storage_error", "lock timeout");
    await Bun.sleep(WAIT_MS);
  }
  try {
    return await fn();
  } finally {
    release(path);
  }
}
