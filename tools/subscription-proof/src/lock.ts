// Cross-process mutual exclusion with a kernel advisory lock: flock(2) on a lock file that is never
// unlinked. The kernel ties ownership to the open file description and drops it when the owner
// closes it or dies, so there is no stale-owner detection or takeover step that could steal a
// live lock. Linux (glibc) only, via Bun's built-in FFI; if flock is unavailable, locking fails
// closed rather than falling back to a weaker scheme.
import { dlopen, FFIType } from "bun:ffi";
import { closeSync, constants, mkdirSync, openSync } from "node:fs";
import { dirname } from "node:path";
import { SafeError } from "./diagnostics";

const LOCK_EX = 2;
const LOCK_NB = 4;
const LOCK_UN = 8;
const WAIT_MS = 25;
const TIMEOUT_MS = 30_000;

type Flock = (fd: number, operation: number) => number;

let flockFn: Flock | null | undefined;
function flock(): Flock {
  if (flockFn === undefined) {
    try {
      const libc = dlopen("libc.so.6", { flock: { args: [FFIType.i32, FFIType.i32], returns: FFIType.i32 } });
      flockFn = (fd, op) => libc.symbols.flock(fd, op);
    } catch {
      flockFn = null;
    }
  }
  if (flockFn === null) throw new SafeError("storage_error", "flock unavailable; refusing to run without a cross-process lock");
  return flockFn;
}

/** Opens the lock file (created once, never removed) and returns its descriptor. */
function open(path: string): number {
  mkdirSync(dirname(path), { recursive: true });
  return openSync(path, constants.O_RDWR | constants.O_CREAT, 0o600);
}

/** One non-blocking attempt; true when this descriptor now owns the lock. */
const tryLock = (fd: number) => flock()(fd, LOCK_EX | LOCK_NB) === 0;

function unlock(fd: number) {
  flock()(fd, LOCK_UN);
  closeSync(fd);
}

/** Runs `fn` while holding the lock at `path`, blocking the thread while waiting. For short critical sections. */
export function withFileLockSync<T>(path: string, fn: () => T, opts: { timeoutMs?: number } = {}): T {
  const fd = open(path);
  const deadline = Date.now() + (opts.timeoutMs ?? TIMEOUT_MS);
  const sleeper = new Int32Array(new SharedArrayBuffer(4));
  try {
    while (!tryLock(fd)) {
      if (Date.now() > deadline) throw new SafeError("storage_error", "lock timeout");
      Atomics.wait(sleeper, 0, 0, WAIT_MS);
    }
  } catch (e) {
    closeSync(fd);
    throw e;
  }
  try {
    return fn();
  } finally {
    unlock(fd);
  }
}

/** Runs async `fn` while holding the lock at `path`, yielding while waiting. */
export async function withFileLock<T>(path: string, fn: () => Promise<T>, opts: { timeoutMs?: number } = {}): Promise<T> {
  const fd = open(path);
  const deadline = Date.now() + (opts.timeoutMs ?? TIMEOUT_MS);
  try {
    while (!tryLock(fd)) {
      if (Date.now() > deadline) throw new SafeError("storage_error", "lock timeout");
      await Bun.sleep(WAIT_MS);
    }
  } catch (e) {
    closeSync(fd);
    throw e;
  }
  try {
    return await fn();
  } finally {
    unlock(fd);
  }
}
