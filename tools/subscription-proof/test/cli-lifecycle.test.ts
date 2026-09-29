// End-to-end CLI checks against a local mock of the OpenAI endpoints (test/mock-openai.ts).
// No real network, credentials, or OS keyring: NZUBE_PROOF_KEYRING=memory keeps credentials in
// process memory, and endpoint overrides are loopback-only by construction.
import { afterAll, beforeAll, describe, expect, setDefaultTimeout, test } from "bun:test";

// Each case spawns real CLI processes.
setDefaultTimeout(30_000);
import { existsSync, mkdtempSync, readFileSync, readdirSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { applyRefresh, type Credential, PLAN_SCOPE, refreshSerialized } from "../src/credentials";
import { memoryStore } from "../src/keyring";
import { createSseParser } from "../src/sse";
import { type Framing, freePort, startMockOpenAI } from "./mock-openai";

const root = join(import.meta.dir, "..");
let mock: Awaited<ReturnType<typeof startMockOpenAI>>;
beforeAll(async () => {
  mock = await startMockOpenAI();
});
afterAll(() => mock.stop());

function env(data: string, port = freePort()) {
  return {
    ...process.env,
    NZUBE_PROOF_DATA: data,
    NZUBE_PROOF_ISSUER: mock.base,
    NZUBE_PROOF_API_BASE: mock.apiBase,
    NZUBE_PROOF_CALLBACK_PORT: String(port),
    NZUBE_PROOF_KEYRING: "memory",
  };
}

/** Starts `signin`, waits for its authorize URL, and returns the process plus URL parameters. */
async function startSignin(data: string) {
  const port = freePort();
  const proc = Bun.spawn(["bun", "src/cli.ts", "signin"], { cwd: root, env: env(data, port), stdout: "pipe", stderr: "pipe" });
  const action = join(data, "signin-action.md");
  for (let i = 0; i < 100 && !(existsSync(action) && readFileSync(action, "utf8").includes("authorize?")); i++) await Bun.sleep(50);
  const url = new URL(readFileSync(action, "utf8").match(/http\S+authorize\?\S+/)?.[0] ?? "");
  mock.state.nonce = url.searchParams.get("nonce") ?? "";
  return { proc, port, state: url.searchParams.get("state") ?? "" };
}

/** Sends the browser redirect; the listener must answer it cleanly (a reset shows the user an error page). */
async function callback(url: string) {
  expect((await fetch(url)).status).toBe(200);
}

/** Exit code, or null when the process is still running after `ms`. */
async function exitWithin(proc: ReturnType<typeof Bun.spawn>, ms: number): Promise<number | null> {
  const code = await Promise.race([proc.exited, Bun.sleep(ms).then(() => null)]);
  if (code === null) proc.kill();
  return code;
}

describe("signin lifecycle", () => {
  test("wrong-state callback fails with nonzero status and exits promptly", async () => {
    const data = mkdtempSync(join(tmpdir(), "nzube-signin-"));
    const s = await startSignin(data);
    await callback(`http://127.0.0.1:${s.port}/auth/callback?state=wrong&code=x`);
    const code = await exitWithin(s.proc, 5000);
    expect(code).not.toBeNull();
    expect(code).not.toBe(0);
  });

  test("denied consent fails with nonzero status and exits promptly", async () => {
    const data = mkdtempSync(join(tmpdir(), "nzube-signin-"));
    const s = await startSignin(data);
    await callback(`http://127.0.0.1:${s.port}/auth/callback?state=${s.state}&error=access_denied`);
    const code = await exitWithin(s.proc, 5000);
    expect(code).not.toBeNull();
    expect(code).not.toBe(0);
  });

  test("successful callback validates the ID token, exits 0 promptly and records plan usage", async () => {
    const data = mkdtempSync(join(tmpdir(), "nzube-signin-"));
    const s = await startSignin(data);
    await callback(`http://127.0.0.1:${s.port}/auth/callback?state=${s.state}&code=c1&client_id=oaiapp_test`);
    expect(await exitWithin(s.proc, 5000)).toBe(0);
    const evidence = JSON.parse(readFileSync(join(data, "evidence.json"), "utf8"));
    const last = evidence.signin.at(-1);
    expect(last.outcome).toBe("signed-in-with-plan-usage");
    expect(Object.values(last.idToken).every(Boolean)).toBe(true);
  });

  test("grant without the plan scope exits nonzero", async () => {
    const data = mkdtempSync(join(tmpdir(), "nzube-signin-"));
    mock.state.grantedScope = "openid profile email offline_access";
    try {
      const s = await startSignin(data);
      await callback(`http://127.0.0.1:${s.port}/auth/callback?state=${s.state}&code=c1&client_id=oaiapp_test`);
      const code = await exitWithin(s.proc, 5000);
      expect(code).not.toBeNull();
      expect(code).not.toBe(0);
    } finally {
      mock.state.grantedScope = `openid profile email offline_access resource.invoke ${PLAN_SCOPE}`;
    }
  });
});

/** Seeds a store with the fixtures and returns the data dir. */
function seededData() {
  const data = mkdtempSync(join(tmpdir(), "nzube-infer-"));
  const e = env(data);
  Bun.spawnSync(["bun", "src/cli.ts", "import", "fixtures/guidance.md", "--name", "g"], { cwd: root, env: e });
  Bun.spawnSync(["bun", "src/cli.ts", "request", "fixtures/request.md", "--select", "g@v1"], { cwd: root, env: e });
  return data;
}

// `--invalid-auth` sends a fixed bearer without a stored credential; the mock ignores auth, so these
// drive the real streaming path end to end. Async spawn: the mock runs in this process.
async function infer(data: string) {
  const p = Bun.spawn(["bun", "src/cli.ts", "infer", "req-1", "--invalid-auth"], { cwd: root, env: env(data), stdout: "ignore", stderr: "ignore" });
  return { exitCode: await p.exited };
}
const outputs = (data: string, kind: "output" | "partial") =>
  existsSync(join(data, "outputs", "req-1")) ? readdirSync(join(data, "outputs", "req-1")).filter((f) => f.endsWith(`.${kind}.md`)) : [];

describe("infer streaming through the CLI", () => {
  for (const framing of ["lf", "crlf", "cr"] as Framing[]) {
    test(`${framing.toUpperCase()}-framed SSE completes with the exact text`, async () => {
      mock.state.framing = framing;
      mock.state.mode = "complete";
      const data = seededData();
      expect((await infer(data)).exitCode).toBe(0);
      const [file] = outputs(data, "output");
      expect(readFileSync(join(data, "outputs", "req-1", file), "utf8")).toBe(mock.state.text);
    });
  }

  test("response.failed exits nonzero and keeps the partial, no complete output", async () => {
    mock.state.framing = "crlf";
    mock.state.mode = "failed";
    const data = seededData();
    expect((await infer(data)).exitCode).not.toBe(0);
    expect(outputs(data, "output")).toEqual([]);
    expect(outputs(data, "partial").length).toBe(1);
  });

  test("stream cut without a terminal event exits nonzero and keeps the partial", async () => {
    mock.state.framing = "lf";
    mock.state.mode = "cut";
    const data = seededData();
    expect((await infer(data)).exitCode).not.toBe(0);
    expect(outputs(data, "output")).toEqual([]);
    expect(outputs(data, "partial").length).toBe(1);
  });
});

describe("concurrent runs", () => {
  test("overlapping infer runs keep every attempt with a unique id", async () => {
    const data = mkdtempSync(join(tmpdir(), "nzube-concurrent-"));
    const e = { ...process.env, NZUBE_PROOF_DATA: data, NZUBE_PROOF_KEYRING: "memory" };
    Bun.spawnSync(["bun", "src/cli.ts", "import", "fixtures/guidance.md", "--name", "g"], { cwd: root, env: e });
    Bun.spawnSync(["bun", "src/cli.ts", "request", "fixtures/request.md", "--select", "g@v1"], { cwd: root, env: e });
    // No credential: each run creates an attempt, fails fast, and finalizes it.
    const runs = Array.from({ length: 8 }, () => Bun.spawn(["bun", "src/cli.ts", "infer", "req-1"], { cwd: root, env: e, stdout: "ignore", stderr: "ignore" }));
    await Promise.all(runs.map((r) => r.exited));
    const store = JSON.parse(readFileSync(join(data, "store", "store.json"), "utf8"));
    const attempts = store.requests[0].attempts as Array<{ id: string; outcome: string }>;
    expect(attempts.length).toBe(8);
    expect(new Set(attempts.map((a) => a.id)).size).toBe(8);
    expect(attempts.every((a) => a.outcome === "failed")).toBe(true);
  });

  test("overlapping successful runs keep one evidence entry per generated attempt", async () => {
    mock.state.framing = "lf";
    mock.state.mode = "complete";
    const data = seededData();
    const runs = Array.from({ length: 40 }, () =>
      Bun.spawn(["bun", "src/cli.ts", "infer", "req-1", "--invalid-auth"], { cwd: root, env: env(data), stdout: "ignore", stderr: "ignore" }),
    );
    expect(await Promise.all(runs.map((r) => r.exited))).toEqual(Array(40).fill(0));
    const store = JSON.parse(readFileSync(join(data, "store", "store.json"), "utf8"));
    const generated = (store.requests[0].attempts as Array<{ id: string; outcome: string }>).filter((a) => a.outcome === "generated").map((a) => a.id);
    const evidence = JSON.parse(readFileSync(join(data, "evidence.json"), "utf8"));
    const recorded = (evidence.infer as Array<{ attemptId: string; outcome: string }>).filter((r) => r.outcome === "generated").map((r) => r.attemptId);
    expect(generated.length).toBe(40);
    expect([...recorded].sort()).toEqual([...generated].sort());
  });

  test("concurrent refreshes of one session exchange the refresh token once", async () => {
    const store = memoryStore();
    const lockDir = mkdtempSync(join(tmpdir(), "nzube-lock-"));
    const expired: Credential = {
      client_id: "oaiapp_test",
      subject: "s",
      ext_agent_host_id: "urn:uuid:placeholder",
      id_token: "placeholder-id",
      access_token: "old",
      refresh_token: "r0",
      token_type: "Bearer",
      scopes: ["openid", PLAN_SCOPE],
      expires_at: 0,
      saved_at: "t0",
    };
    await store.save("oaiapp_test", expired);
    let exchanges = 0;
    const refresh = async (cred: Credential) => {
      exchanges += 1;
      await Bun.sleep(100);
      return { access_token: `new-from-${cred.refresh_token}`, refresh_token: "r1", expires_in: 3600, scope: `openid ${PLAN_SCOPE}` };
    };
    const opts = { clientId: "oaiapp_test", lockPath: join(lockDir, "refresh.lock"), store, refresh, nowSeconds: () => 1_000 };
    const [a, b] = await Promise.all([refreshSerialized(opts), refreshSerialized(opts)]);
    expect(exchanges).toBe(1);
    expect(a.access_token).toBe("new-from-r0");
    expect(b.access_token).toBe("new-from-r0");
    expect(applyRefresh).toBeDefined();
  });
});

describe("SSE parser", () => {
  const events = ['{"type":"a"}', '{"type":"b","delta":"x"}'];
  for (const [name, eol] of [["LF", "\n"], ["CRLF", "\r\n"], ["CR", "\r"]] as const) {
    test(`${name} framing at every chunk size yields the same data`, () => {
      const wire = events.map((d) => `event: e${eol}data: ${d}${eol}${eol}`).join("");
      for (let size = 1; size <= wire.length; size++) {
        const p = createSseParser();
        const got: string[] = [];
        for (let i = 0; i < wire.length; i += size) got.push(...p.push(wire.slice(i, i + size)));
        got.push(...p.end());
        expect(got).toEqual(events);
      }
    });
  }
});
