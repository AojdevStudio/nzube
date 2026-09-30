// Regression checks for the subscription proof. Placeholder strings only; no real tokens.
import { describe, expect, test } from "bun:test";
import { settle } from "../src/outcome";
import { applyRefresh, checkIdentityBinding, type Credential, PLAN_SCOPE, subjectBinding } from "../src/credentials";

const clean = { completed: true, text: "brief", partialText: "brief", unsafeEvents: [], failure: null };

describe("settle", () => {
  test("clean completion commits the output", () => {
    expect(settle(clean)).toEqual({ outcome: "generated", output: "brief" });
  });
  test("tool events fail closed and commit no output", () => {
    const s = settle({ ...clean, unsafeEvents: ["commandExecution"] });
    expect(s.outcome).toBe("failed");
    expect("output" in s).toBe(false);
  });
  test("tool events keep the text as a separate partial", () => {
    const s = settle({ ...clean, unsafeEvents: ["serverRequest:item/commandExecution/requestApproval"] });
    expect(s).toMatchObject({ outcome: "failed", partial: "brief" });
  });
  test("interrupted stream retains streamed text as partial", () => {
    const s = settle({ completed: false, text: null, partialText: "half a bri", unsafeEvents: [], failure: "interrupted" });
    expect(s).toEqual({ outcome: "failed", error: "interrupted", partial: "half a bri" });
  });
  test("empty partial is not retained", () => {
    const s = settle({ completed: false, text: null, partialText: "", unsafeEvents: [], failure: "failed" });
    expect(s).toMatchObject({ partial: null });
  });
});

describe("identity binding", () => {
  test("first registration has no binding to check", () => {
    expect(() => checkIdentityBinding(null, "sub-a")).not.toThrow();
  });
  test("same subject passes", () => {
    expect(() => checkIdentityBinding(subjectBinding("sub-a"), "sub-a")).not.toThrow();
  });
  test("different subject is rejected before credentials are replaced", () => {
    expect(() => checkIdentityBinding(subjectBinding("sub-a"), "sub-b")).toThrow(/identity/);
  });
});

describe("refresh scope", () => {
  const prev: Credential = {
    client_id: "oaiapp_placeholder",
    subject: "sub-a",
    ext_agent_host_id: "urn:uuid:placeholder",
    id_token: "placeholder-id",
    access_token: "placeholder-old",
    refresh_token: "placeholder-refresh-old",
    token_type: "Bearer",
    scopes: ["openid", "offline_access", PLAN_SCOPE],
    expires_at: 0,
    saved_at: "t0",
  };
  test("refresh keeping the plan scope is applied", () => {
    const next = applyRefresh(prev, { access_token: "placeholder-new", refresh_token: "placeholder-refresh-new", scope: `openid offline_access ${PLAN_SCOPE}`, expires_in: 3600 }, 100, "t1");
    expect(next.access_token).toBe("placeholder-new");
    expect(next.scopes).toContain(PLAN_SCOPE);
  });
  test("refresh that drops the plan scope is rejected", () => {
    expect(() =>
      applyRefresh(prev, { access_token: "placeholder-new", refresh_token: "placeholder-refresh-new", scope: "openid offline_access", expires_in: 3600 }, 100, "t1"),
    ).toThrow(PLAN_SCOPE);
  });
  test("refresh without a scope field retains the prior grant", () => {
    const next = applyRefresh(prev, { access_token: "placeholder-new", expires_in: 3600 }, 100, "t1");
    expect(next.scopes).toEqual(prev.scopes);
  });
});

// ---------------------------------------------------------------------------------------------
// Non-2xx handling, exit codes, attempt history.
import { mkdtempSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { httpFailure, modelsFromResponse } from "../src/http";
import { attemptFile, type RequestRecord } from "../src/store";

describe("SIWC HTTP failures", () => {
  test("persisted failure has status and code but no body text", () => {
    const f = httpFailure(401, JSON.stringify({ detail: "rejected for person@example.com with secret context" }), "req_1");
    expect(f).toMatchObject({ status: 401, requestId: "req_1", code: null });
    expect(JSON.stringify(f)).not.toContain("person@example.com");
    expect(JSON.stringify(f)).not.toContain("secret context");
  });
  test("Responses error code is kept, message dropped", () => {
    const f = httpFailure(429, JSON.stringify({ error: { code: "subscription_sharing_usage_limit_exceeded", type: "usage", message: "private message" } }), null);
    expect(f.code).toBe("subscription_sharing_usage_limit_exceeded");
    expect(JSON.stringify(f)).not.toContain("private message");
  });
  test("OAuth error code is kept, description dropped", () => {
    const f = httpFailure(400, JSON.stringify({ error: "invalid_grant", error_description: "private description" }), null);
    expect(f.code).toBe("invalid_grant");
    expect(JSON.stringify(f)).not.toContain("private description");
  });
  test("non-2xx /models is a failure, not an empty model list", () => {
    const r = modelsFromResponse(401, JSON.stringify({ detail: "no" }), "req_2");
    expect(r.ok).toBe(false);
  });
  test("2xx /models lists display models", () => {
    const r = modelsFromResponse(200, JSON.stringify({ models: [{ slug: "m1", visibility: "list" }, { slug: "m2", visibility: "hide" }] }), null);
    expect(r).toEqual({ ok: true, listed: ["m1"] });
  });
});

describe("attempt history", () => {
  const req = { id: "req-1", attempts: [] } as unknown as RequestRecord;
  test("each attempt gets its own output and partial file", () => {
    const a1 = { id: "a1-x" } as RequestRecord["attempts"][number];
    const a2 = { id: "a2-y" } as RequestRecord["attempts"][number];
    const paths = [attemptFile(req, a1, "output"), attemptFile(req, a2, "output"), attemptFile(req, a1, "partial"), attemptFile(req, a2, "partial")];
    expect(new Set(paths).size).toBe(4);
    for (const p of paths) expect(p.endsWith("/output.md") || p.endsWith("/output.partial.md")).toBe(false);
  });
  test("writing a second attempt keeps the first attempt's text", async () => {
    const data = mkdtempSync(join(tmpdir(), "nzube-test-"));
    const script = `import { newAttempt, writeAttemptText } from "${import.meta.dir}/../src/store";
      const req = { id: "req-1", attempts: [] };
      const a1 = newAttempt(req, "siwc-direct", "h", []); const p1 = writeAttemptText(req, a1, "output", "first");
      const a2 = newAttempt(req, "siwc-direct", "h", []); const p2 = writeAttemptText(req, a2, "output", "second");
      console.log(JSON.stringify([p1, p2]));`;
    const p = Bun.spawnSync(["bun", "-e", script], { env: { ...process.env, NZUBE_PROOF_DATA: data } });
    const [p1, p2] = JSON.parse(p.stdout.toString()) as [string, string];
    expect(p1).not.toBe(p2);
    expect(readFileSync(p1, "utf8")).toBe("first");
    expect(readFileSync(p2, "utf8")).toBe("second");
  });
});

describe("SIWC CLI exit codes", () => {
  const root = join(import.meta.dir, "..");
  const data = mkdtempSync(join(tmpdir(), "nzube-test-data-"));
  const env = { ...process.env, NZUBE_PROOF_DATA: data };
  const run = (args: string[]) => Bun.spawnSync(["bun", ...args], { cwd: root, env });
  run(["src/cli.ts", "import", "fixtures/guidance.md", "--name", "g"]);
  run(["src/cli.ts", "request", "fixtures/request.md", "--select", "g@v1"]);

  test("failed infer exits nonzero and keeps every attempt and the raw request", () => {
    expect(run(["src/cli.ts", "infer", "req-1"]).exitCode).not.toBe(0);
    expect(run(["src/cli.ts", "infer", "req-1"]).exitCode).not.toBe(0);
    const store = JSON.parse(readFileSync(join(data, "store", "store.json"), "utf8")) as { requests: RequestRecord[] };
    const req = store.requests[0];
    expect(req.attempts.map((a) => a.outcome)).toEqual(["failed", "failed"]);
    expect(new Set(req.attempts.map((a) => a.id)).size).toBe(2);
    expect(new Bun.CryptoHasher("sha256").update(req.raw).digest("hex")).toBe(req.rawSha256);
  });
  test("check without a usable credential exits nonzero", () => {
    expect(run(["src/cli.ts", "check"]).exitCode).not.toBe(0);
  });
});

// ---------------------------------------------------------------------------------------------
// Diagnostics persist only categories, allowlisted codes and numbers.
import { errorDiagnostic, providerCode } from "../src/diagnostics";

const person = "customer Alice Smith";
const phone = "+1 555 010 0199";

describe("allowlisted diagnostics", () => {
  test("unknown server-supplied code is replaced, not stored", () => {
    expect(providerCode(person)).toBe("unknown_provider_error");
  });
  test("documented codes pass through", () => {
    expect(providerCode("subscription_sharing_usage_limit_exceeded")).toBe("subscription_sharing_usage_limit_exceeded");
    expect(providerCode("invalid_grant")).toBe("invalid_grant");
  });
  test("SIWC HTTP failure with a personal code and type serializes without it", () => {
    const f = httpFailure(400, JSON.stringify({ error: { code: person, type: `Bob ${phone}` } }), "req_x");
    const s = JSON.stringify(f);
    expect(s).not.toContain("Alice");
    expect(s).not.toContain("Bob");
    expect(s).not.toContain("555");
    expect(f.code).toBe("unknown_provider_error");
  });
  test("OAuth error string with personal text serializes without it", () => {
    expect(JSON.stringify(httpFailure(400, JSON.stringify({ error: person }), null))).not.toContain("Alice");
  });
  test("thrown error text (stderr, rpc message) is not persisted", () => {
    const d = errorDiagnostic(new Error(`provider stderr: ${person} called from ${phone}`));
    const s = JSON.stringify(d);
    expect(s).not.toContain("Alice");
    expect(s).not.toContain("555");
    expect(typeof d.category).toBe("string");
  });
});
