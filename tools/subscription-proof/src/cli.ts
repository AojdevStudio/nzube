// Nzube subscription proof: direct "Sign in with ChatGPT" (OpenAI OSS token sharing) CLI.
// Public client: PKCE S256 + dynamic registration (client_id=dynamic_agent_client), loopback
// callback on 127.0.0.1:1455, no client secret, no API key, no billing fallback. Tokens live only
// in the OS keyring; the data directory holds non-secret metadata, requests and outputs.
// Docs: https://developers.openai.com/siwc/token-sharing-open-source/ (sign-in, profiles-and-sessions,
// models-and-inference, errors-and-recovery, preview-limitations).
//
//   bun src/cli.ts selftest                       PKCE RFC 7636 vector, keyring round trip, OIDC discovery
//   bun src/cli.ts import <file> --name <name>    import guidance (content-addressed, versioned)
//   bun src/cli.ts request <file> [--select <id>] persist a raw request with explicit guidance selection
//   bun src/cli.ts signin                         browser consent; validate; store credential in keyring
//   bun src/cli.ts check                          GET /v1/models with the stored credential
//   bun src/cli.ts infer <requestId> [--cut-after N] [--invalid-auth]
//                                                 one streamed Responses call; complete output or kept partial
//   bun src/cli.ts export <requestId>             export latest complete output; prove byte equality
//   bun src/cli.ts show <requestId>               print the persisted request and attempts
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join, relative } from "node:path";
import type { JsonValue } from "./json";
import { settle } from "./outcome";
import { allowlisted, errorDiagnostic, errorSummary, providerCode, SafeError, shapedCounts } from "./diagnostics";

/** Server-supplied values persisted only when known. */
const responseItemTypes = new Set([
  "message", "reasoning", "function_call", "custom_tool_call", "web_search_call", "file_search_call", "computer_call",
  "code_interpreter_call", "image_generation_call", "mcp_call", "mcp_list_tools", "mcp_approval_request", "local_shell_call", "tool_search_call",
]);
const knownScopes = new Set(["openid", "profile", "email", "offline_access", "resource.invoke", "chatgpt.tokens.use.direct"]);
const tokenFields = new Set(["access_token", "refresh_token", "id_token", "token_type", "expires_in", "scope", "earliest_refresh_at"]);
/** Known scopes as granted, plus a count of any others. */
const scopeSummary = (granted: string[]) => ({ known: granted.filter((s) => knownScopes.has(s)), unknownCount: granted.filter((s) => !knownScopes.has(s)).length });
import { createRequest, dataRoot, findRequest, finishAttempt, generatorInstructions, importSource, loadStore, now, sha256, startAttempt, writeAtomic, writeAttemptText } from "./store";
import { selectedStore } from "./keyring";
import { API_BASE, CALLBACK_PORT, ISSUER, RESOURCE } from "./endpoints";

const keyring = selectedStore();
import { httpFailure, modelsFromResponse } from "./http";
import { checkIdentityBinding, type Credential, PLAN_SCOPE, refreshSerialized, subjectBinding } from "./credentials";
import { createSseParser } from "./sse";
import { withFileLockSync } from "./lock";

type Obj = { [k: string]: JsonValue };

const AGENT_NAME = "Nzube"; // agent_name_hint: the app's actual name, same on every install
const SCOPES = "openid profile email offline_access resource.invoke chatgpt.tokens.use.direct";
const REDIRECT_URI = `http://127.0.0.1:${CALLBACK_PORT}/auth/callback`;

const dataDir = dataRoot;
const stateDir = join(dataDir, "state");
const hostFile = join(stateDir, "host.json");
const registrationFile = join(stateDir, "registration.json");
const evidenceFile = join(dataDir, "evidence.json");
const actionFile = join(dataDir, "signin-action.md");

/** Reads a body once; JSON when it parses, else an empty object. */
async function readBody(res: Response): Promise<{ text: string; json: Obj }> {
  const text = await res.text();
  try {
    const json = JSON.parse(text) as JsonValue;
    return { text, json: json && typeof json === "object" && !Array.isArray(json) ? json : {} };
  } catch {
    return { text, json: {} };
  }
}

// ---------------------------------------------------------------------------------------------
// Small primitives

const b64url = (bytes: Uint8Array) => Buffer.from(bytes).toString("base64url");
const randomToken = () => b64url(crypto.getRandomValues(new Uint8Array(32)));

async function pkceChallenge(verifier: string): Promise<string> {
  return b64url(new Uint8Array(await crypto.subtle.digest("SHA-256", new TextEncoder().encode(verifier))));
}

/** Appends one record to a section of evidence.json. Overlapping runs serialize on the evidence lock. */
function record(section: string, value: JsonValue) {
  withFileLockSync(`${evidenceFile}.lock`, () => {
    const current = existsSync(evidenceFile) ? (JSON.parse(readFileSync(evidenceFile, "utf8")) as Obj) : {};
    const prior = current[section];
    current[section] = [...(Array.isArray(prior) ? prior : []), value];
    writeAtomic(evidenceFile, `${JSON.stringify(current, null, 2)}\n`);
  });
}

/** Stable opaque host id (urn:uuid v4), created once before the first sign-in. */
function hostId(): string {
  mkdirSync(stateDir, { recursive: true, mode: 0o700 });
  if (existsSync(hostFile)) return (JSON.parse(readFileSync(hostFile, "utf8")) as { ext_agent_host_id: string }).ext_agent_host_id;
  const id = `urn:uuid:${crypto.randomUUID()}`;
  writeAtomic(hostFile, `${JSON.stringify({ ext_agent_host_id: id, createdAt: now() }, null, 2)}\n`);
  return id;
}

type Discovery = { issuer: string; authorization_endpoint: string; token_endpoint: string; jwks_uri: string; revocation_endpoint: string };
async function discover(): Promise<Discovery> {
  const d = (await (await fetch(`${ISSUER}/.well-known/openid-configuration`)).json()) as Discovery;
  // The docs name these endpoints explicitly; refuse to continue if discovery disagrees.
  if (d.issuer !== ISSUER || d.authorization_endpoint !== `${ISSUER}/api/accounts/authorize` || d.token_endpoint !== `${ISSUER}/api/accounts/oauth/token`)
    throw new SafeError("auth", "OIDC discovery does not match the documented endpoints");
  return d;
}

// ---------------------------------------------------------------------------------------------

// ---------------------------------------------------------------------------------------------
// ID token validation (RS256 against OpenAI's JWKS, iss, aud, exp, nonce)

type IdClaims = { iss: string; aud: string | string[]; exp: number; nonce?: string; sub: string };
async function validateIdToken(idToken: string, jwksUri: string, clientId: string, nonce: string) {
  const [h, p, s] = idToken.split(".");
  const header = JSON.parse(Buffer.from(h, "base64url").toString()) as { alg: string; kid: string };
  if (header.alg !== "RS256") throw new SafeError("auth", "unexpected id_token alg");
  const { keys } = (await (await fetch(jwksUri)).json()) as { keys: Array<JsonWebKey & { kid: string }> };
  const jwk = keys.find((k) => k.kid === header.kid);
  if (!jwk) throw new SafeError("auth", "id_token kid not in JWKS");
  const key = await crypto.subtle.importKey("jwk", jwk, { name: "RSASSA-PKCS1-v1_5", hash: "SHA-256" }, false, ["verify"]);
  const signatureValid = await crypto.subtle.verify("RSASSA-PKCS1-v1_5", key, Buffer.from(s, "base64url"), new TextEncoder().encode(`${h}.${p}`));
  const claims = JSON.parse(Buffer.from(p, "base64url").toString()) as IdClaims;
  const checks = {
    signatureValid,
    issuerOk: claims.iss === ISSUER,
    audienceOk: Array.isArray(claims.aud) ? claims.aud.includes(clientId) : claims.aud === clientId,
    notExpired: claims.exp > Date.now() / 1000,
    nonceOk: claims.nonce === nonce,
  };
  if (!Object.values(checks).every(Boolean)) throw new SafeError("auth", `id_token validation failed: ${JSON.stringify(checks)}`);
  return { checks, subject: claims.sub };
}

// ---------------------------------------------------------------------------------------------
// Commands

async function selftest() {
  // RFC 7636 appendix B vector.
  const pkceOk = (await pkceChallenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk")) === "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";
  const probe = { client_id: "selftest", subject: "", ext_agent_host_id: "", id_token: "", access_token: "dummy", refresh_token: null, token_type: "Bearer", scopes: [], expires_at: 0, saved_at: now() } satisfies Credential;
  await keyring.save("selftest", probe);
  const roundTrip = (await keyring.load("selftest"))?.access_token === "dummy";
  await keyring.remove("selftest");
  const cleared = (await keyring.load("selftest")) === null;
  const d = await discover();
  const result = { at: now(), pkceRfc7636Vector: pkceOk, keyringRoundTrip: roundTrip, keyringCleared: cleared, discovery: d, hostIdFormat: hostId().split(":").slice(0, 2).join(":") };
  record("selftest", result);
  console.log(JSON.stringify(result, null, 2));
}

async function signin() {
  const d = await discover();
  const host = hostId();
  const reg = existsSync(registrationFile)
    ? (JSON.parse(readFileSync(registrationFile, "utf8")) as { client_id: string; subjectSha256: string | null })
    : null;
  const state = randomToken();
  const nonce = randomToken();
  const verifier = randomToken();
  const clientIdForRequest = reg?.client_id ?? "dynamic_agent_client";
  const params = new URLSearchParams({
    client_id: clientIdForRequest,
    ...(reg ? {} : { agent_name_hint: AGENT_NAME }),
    ext_agent_host_id: host,
    response_type: "code",
    redirect_uri: REDIRECT_URI,
    scope: SCOPES,
    resource: RESOURCE,
    state,
    nonce,
    code_challenge_method: "S256",
    code_challenge: await pkceChallenge(verifier),
  });
  const authorizeUrl = `${d.authorization_endpoint}?${params}`;

  let deliverCallback: (v: URLSearchParams) => void = () => {};
  const callback = new Promise<URLSearchParams>((r) => (deliverCallback = r));
  const server = Bun.serve({
    hostname: "127.0.0.1",
    port: CALLBACK_PORT,
    fetch(req) {
      const url = new URL(req.url);
      if (url.pathname !== "/auth/callback") return new Response("not found", { status: 404 });
      deliverCallback(url.searchParams);
      return new Response("Nzube subscription proof: sign-in received. You can close this tab.");
    },
  });

  writeFileSync(
    actionFile,
    [
      "# Direct Sign in with ChatGPT (Nzube subscription proof)",
      "",
      `Started: ${now()} (listener waits up to 30 minutes)`,
      `Mode: ${reg ? "reauthorization with issued client_id" : "first-time dynamic registration (client_id=dynamic_agent_client)"}`,
      "",
      `Open this URL in a browser on THIS machine (the callback is ${REDIRECT_URI}, which only reaches the local host):`,
      "",
      authorizeUrl,
      "",
      "Then: sign in to ChatGPT, pick the workspace, keep the agent name (Nzube), and approve use of the ChatGPT plan.",
      "The URL carries only public PKCE/state values; the verifier stays in the waiting process.",
      "",
    ].join("\n"),
    { mode: 0o600 },
  );
  console.log("WAITING");

  let timer: ReturnType<typeof setTimeout> | undefined;
  const timeout = new Promise<never>((_, reject) => {
    timer = setTimeout(() => reject(new SafeError("timeout", "no callback within 30 minutes")), 30 * 60_000);
  });
  const evidence: Obj = { at: now(), mode: reg ? "reauth" : "dynamic-registration", redirectUri: REDIRECT_URI, requestedScopes: SCOPES, resource: RESOURCE };
  try {
    const q = await Promise.race([callback, timeout]);
    if (q.get("state") !== state) throw new SafeError("auth", "state mismatch");
    if (q.get("error")) {
      evidence.callback = { error: providerCode(q.get("error")) };
      throw new SafeError("auth", "authorization returned an error; ChatGPT plan use not enabled, no fallback");
    }
    const issued = reg?.client_id ?? q.get("client_id");
    if (!issued || issued === "dynamic_agent_client") throw new SafeError("auth", "registration incomplete: callback carried no issued client_id");
    if (reg && q.get("client_id") && q.get("client_id") !== reg.client_id) throw new SafeError("auth", "callback client_id differs from saved registration");
    evidence.callback = { stateOk: true, issuedClientIdPrefix: issued.startsWith("oaiapp_") ? "oaiapp" : "other" };

    const res = await fetch(d.token_endpoint, {
      method: "POST",
      headers: { "content-type": "application/x-www-form-urlencoded" },
      body: new URLSearchParams({ grant_type: "authorization_code", client_id: issued, code: q.get("code") ?? "", code_verifier: verifier, redirect_uri: REDIRECT_URI, resource: RESOURCE }),
    });
    const { text: tokText, json: tok } = await readBody(res);
    if (!res.ok) {
      const failure = httpFailure(res.status, tokText, res.headers.get("x-request-id"));
      evidence.tokenExchange = failure;
      throw new SafeError("http_error", `token exchange ${failure.status} ${failure.code ?? ""}`.trim());
    }
    evidence.tokenExchange = { status: res.status, fields: Object.keys(tok).filter((k) => tokenFields.has(k)).sort(), unknownFieldCount: Object.keys(tok).filter((k) => !tokenFields.has(k)).length };
    const granted = String(tok.scope ?? "").split(" ").filter(Boolean);
    const { checks, subject } = await validateIdToken(tok.id_token as string, d.jwks_uri, issued, nonce);
    evidence.idToken = checks;
    evidence.grantedScopes = scopeSummary(granted);
    evidence.planUsageGranted = granted.includes(PLAN_SCOPE);
    // Official sign-in docs: confirm the validated identity matches the selected account before
    // replacing credentials. Throws (leaving keyring and registration untouched) on mismatch.
    checkIdentityBinding(reg?.subjectSha256 ?? null, subject);
    evidence.identityBinding = reg ? "matched existing registration" : "new registration";

    await keyring.save(issued, {
      client_id: issued,
      subject,
      ext_agent_host_id: host,
      id_token: tok.id_token as string,
      access_token: tok.access_token as string,
      refresh_token: (tok.refresh_token as string | undefined) ?? null,
      token_type: String(tok.token_type ?? "Bearer"),
      scopes: granted,
      expires_at: Math.floor(Date.now() / 1000) + Number(tok.expires_in ?? 0),
      saved_at: now(),
    });
    writeAtomic(registrationFile, `${JSON.stringify({ client_id: issued, subjectSha256: subjectBinding(subject), agent_name_hint: AGENT_NAME, redirect_uri: REDIRECT_URI, scopes: granted, planUsageGranted: granted.includes(PLAN_SCOPE), savedAt: now(), storage: "secret-service" }, null, 2)}\n`);
    evidence.storage = { keyring: "secret-service", tokensOnDisk: false };
    evidence.outcome = granted.includes(PLAN_SCOPE) ? "signed-in-with-plan-usage" : "signed-in-without-plan-usage (inference blocked)";
  } catch (e) {
    evidence.outcome = "failed";
    evidence.error = errorDiagnostic(e);
  } finally {
    clearTimeout(timer);
    // Let the browser receive its response before connections are closed.
    await Promise.race([server.stop(), Bun.sleep(1_000)]);
    server.stop(true);
    record("signin", evidence);
    writeFileSync(actionFile, `\nResult: ${JSON.stringify({ outcome: evidence.outcome, error: evidence.error ?? null })}\n`, { flag: "a" });
  }
  console.log(JSON.stringify(evidence, null, 2));
  // Only a sign-in that can use the ChatGPT plan counts as success for this proof.
  if (evidence.outcome !== "signed-in-with-plan-usage") process.exitCode = 1;
}

/** Loads the stored credential, refreshing it (rotating refresh token) when within 5 minutes of expiry. */
async function credential(): Promise<Credential> {
  if (!existsSync(registrationFile)) throw new SafeError("auth", "not signed in: run `bun src/cli.ts signin`");
  const { client_id } = JSON.parse(readFileSync(registrationFile, "utf8")) as { client_id: string };
  const cred = await refreshSerialized({
    clientId: client_id,
    lockPath: join(stateDir, "refresh.lock"),
    store: keyring,
    nowSeconds: () => Math.floor(Date.now() / 1000),
    refresh: async (stale) => {
      const d = await discover();
      const res = await fetch(d.token_endpoint, {
        method: "POST",
        headers: { "content-type": "application/x-www-form-urlencoded" },
        body: new URLSearchParams({ grant_type: "refresh_token", client_id, refresh_token: stale.refresh_token ?? "", resource: RESOURCE }),
      });
      const { text: tokText, json: tok } = await readBody(res);
      if (!res.ok) {
        const failure = httpFailure(res.status, tokText, res.headers.get("x-request-id"));
        throw new SafeError("http_error", `refresh ${failure.status} ${failure.code ?? ""}`.trim());
      }
      return tok;
    },
  });
  if (!cred.scopes.includes(PLAN_SCOPE)) throw new SafeError("scope_missing", `${PLAN_SCOPE} not granted; ChatGPT plan use disabled, no fallback`);
  return cred;
}

async function check() {
  const cred = await credential();
  const res = await fetch(`${API_BASE}/models`, { headers: { authorization: `Bearer ${cred.access_token}` } });
  const models = modelsFromResponse(res.status, await res.text(), res.headers.get("x-request-id"));
  const result = { at: now(), status: res.status, ...models };
  record("check", result);
  console.log(JSON.stringify(result, null, 2));
  if (!models.ok) throw new SafeError("http_error", `GET /v1/models ${models.failure.status} ${models.failure.code ?? ""}`.trim());
}

/** Test controls for infer. None changes the provider route; all failures stay failures. */
type InferOptions = {
  /** Abort the HTTP stream after this many text deltas (real client-side stream cut). */
  cutAfter: number | null;
  /** Send a deliberately invalid bearer instead of the stored credential (no fallback expected). */
  invalidAuth: boolean;
};

/** Numeric token counts only from a Responses `usage` object. */
function usageNumbers(usage: JsonValue | undefined): Obj | null {
  if (!usage || typeof usage !== "object" || Array.isArray(usage)) return null;
  const out: Obj = {};
  for (const [k, v] of Object.entries(usage)) {
    if (typeof v === "number") out[k] = v;
    else if (v && typeof v === "object" && !Array.isArray(v))
      for (const [k2, v2] of Object.entries(v)) if (typeof v2 === "number") out[`${k}.${k2}`] = v2;
  }
  return out;
}

const requestIdShape = (id: string | null) => (id && /^[A-Za-z0-9_-]{1,128}$/.test(id) ? id : null);

async function infer(requestId: string, opts: InferOptions = { cutAfter: null, invalidAuth: false }) {
  const { req, assembly, attempt } = startAttempt(requestId, "siwc-direct");
  const evidence: Obj = {
    at: now(),
    route: "siwc-direct",
    endpoint: `${API_BASE}/responses`,
    requestBody: { store: false, stream: true, tools: "absent" },
    requestId,
    attemptId: attempt.id ?? null,
    inputSha256: assembly.sha256,
    usedSources: assembly.included,
    options: { cutAfter: opts.cutAfter, invalidAuth: opts.invalidAuth },
  };
  let text = ""; // streamed deltas; retained separately if the response does not commit
  /** Keeps uncommitted text in this attempt's own partial file; complete output is never touched here. */
  const retainPartial = (partial: string | null) => {
    if (!partial) return;
    const path = writeAttemptText(req, attempt, "partial", partial);
    evidence.partial = { path: relative(dataRoot, path), sha256: sha256(partial), chars: partial.length };
  };
  // Cancellation: keep the partial, mark the attempt cancelled, leave the last complete output alone.
  process.on("SIGINT", () => {
    retainPartial(text);
    attempt.outcome = "cancelled";
    attempt.error = "cancelled: SIGINT";
    attempt.endedAt = now();
    finishAttempt(requestId, attempt);
    evidence.outcome = "cancelled";
    evidence.rawRequestPreserved = sha256(loadStore().requests.find((r) => r.id === requestId)?.raw ?? "") === req.rawSha256;
    record("infer", evidence);
    process.exit(130);
  });
  const abort = new AbortController();
  try {
    const bearer = opts.invalidAuth ? "invalid-test-bearer" : (await credential()).access_token;
    const modelsRes = await fetch(`${API_BASE}/models`, { headers: { authorization: `Bearer ${bearer}` } });
    const models = modelsFromResponse(modelsRes.status, await modelsRes.text(), modelsRes.headers.get("x-request-id"));
    if (!models.ok) {
      evidence.models = models.failure;
      throw new SafeError("http_error", `GET /v1/models ${models.failure.status} ${models.failure.code ?? ""}`.trim());
    }
    const listed = models.listed;
    const model = listed.find((s) => s === "gpt-5.6-luna") ?? listed.find((s) => s.includes("luna"));
    if (!model) throw new SafeError("gate", "no luna-tier model listed for this account");
    evidence.model = model;
    evidence.listedModels = listed;
    // No `tools` field: the request body itself is tool-free.
    const res = await fetch(`${API_BASE}/responses`, {
      method: "POST",
      headers: { authorization: `Bearer ${bearer}`, "content-type": "application/json" },
      body: JSON.stringify({ model, instructions: generatorInstructions, input: [{ role: "user", content: [{ type: "input_text", text: assembly.text }] }], store: false, stream: true }),
      signal: abort.signal,
    });
    evidence.http = { status: res.status, requestId: requestIdShape(res.headers.get("x-request-id")) };
    if (!res.ok || !res.body) {
      const failure = httpFailure(res.status, await res.text(), res.headers.get("x-request-id"));
      evidence.http = failure;
      throw new SafeError("http_error", `POST /v1/responses ${failure.status} ${failure.code ?? ""}`.trim());
    }
    const eventCounts: Record<string, number> = {};
    const outputItemTypes: string[] = [];
    let terminal = null as Obj | null; // assigned in the parser callback
    let deltas = 0;
    const parser = createSseParser();
    const handle = (data: string) => {
      if (data === "[DONE]") return;
      const ev = JSON.parse(data) as Obj;
      const type = String(ev.type);
      eventCounts[type] = (eventCounts[type] ?? 0) + 1;
      if (type === "response.output_text.delta") {
        text += String(ev.delta);
        deltas += 1;
        if (deltas === 1) console.error("STREAMING");
        if (opts.cutAfter !== null && deltas >= opts.cutAfter) abort.abort();
      }
      if (type === "response.output_item.added") outputItemTypes.push(String((ev.item as Obj).type));
      if (type === "response.completed" || type === "response.failed" || type === "response.incomplete") terminal = ev;
    };
    for await (const chunk of res.body.pipeThrough(new TextDecoderStream())) for (const data of parser.push(chunk)) handle(data);
    for (const data of parser.end()) handle(data);
    const response = (terminal?.response ?? {}) as Obj;
    evidence.stream = { eventCounts: shapedCounts(eventCounts), outputItemTypes: outputItemTypes.map((t) => allowlisted(t, responseItemTypes, "unknown_item")), terminal: terminal?.type ?? "stream ended without terminal event", usage: usageNumbers(response.usage), servedModel: allowlisted(response.model, new Set(listed), "unlisted_model"), errorCode: providerCode((response.error as Obj | null)?.code) };
    // Tool items are checked before anything is committed; only a completed, tool-free stream writes output.md.
    const settled = settle({
      completed: terminal?.type === "response.completed",
      text: terminal?.type === "response.completed" ? text : null,
      partialText: text,
      unsafeEvents: outputItemTypes.filter((t) => t !== "message" && t !== "reasoning").map((t) => allowlisted(t, responseItemTypes, "unknown_item")),
      failure: terminal?.type === "response.completed" ? null : `no response.completed: ${terminal?.type ?? "interrupted stream"}`,
    });
    if (settled.outcome === "generated") {
      const path = writeAttemptText(req, attempt, "output", settled.output);
      attempt.outcome = "generated";
      evidence.output = { path: relative(dataRoot, path), sha256: sha256(settled.output), chars: settled.output.length };
      evidence.outcome = "generated";
    } else {
      retainPartial(settled.partial);
      attempt.outcome = "failed";
      attempt.error = settled.error;
      evidence.outcome = "failed";
      evidence.error = settled.error;
    }
  } catch (e) {
    retainPartial(text);
    attempt.outcome = "failed";
    attempt.error = abort.signal.aborted ? "stream_incomplete: client cut the stream" : errorSummary(e);
    evidence.outcome = "failed";
    evidence.error = attempt.error;
  } finally {
    attempt.endedAt = now();
    finishAttempt(requestId, attempt);
    evidence.rawRequestPreserved = sha256(loadStore().requests.find((r) => r.id === requestId)?.raw ?? "") === req.rawSha256;
    record("infer", evidence);
  }
  console.log(JSON.stringify(evidence, null, 2));
  if (evidence.outcome !== "generated") process.exitCode = 1; // chained steps must not see success
}

/**
 * Exports the latest complete SIWC output of a request and proves the exported bytes equal the
 * stored per-attempt output and the text hash recorded when the stream completed.
 */
function exportLatest(requestId: string) {
  const req = findRequest(loadStore(), requestId);
  const attempt = [...req.attempts].reverse().find((a) => a.route === "siwc-direct" && a.outcome === "generated" && a.outputPath);
  if (!attempt?.outputPath) throw new SafeError("gate", "no complete SIWC output to export");
  const stored = readFileSync(attempt.outputPath);
  const exportPath = join(dataRoot, "exports", `${req.id}.${attempt.id}.md`);
  mkdirSync(join(dataRoot, "exports"), { recursive: true });
  writeFileSync(exportPath, stored.toString("utf8"), { mode: 0o600 });
  const exported = readFileSync(exportPath);
  const inferRecords = (existsSync(evidenceFile) ? ((JSON.parse(readFileSync(evidenceFile, "utf8")) as Obj).infer as Obj[] | undefined) : undefined) ?? [];
  const streamedSha = (inferRecords.find((r) => r.attemptId === attempt.id)?.output as Obj | undefined)?.sha256 ?? null;
  const result = {
    at: now(),
    requestId,
    attemptId: attempt.id ?? null,
    exportPath: relative(dataRoot, exportPath),
    bytes: exported.length,
    exportEqualsStored: Buffer.compare(exported, stored) === 0,
    exportSha256: sha256(exported),
    matchesStreamedTextSha: streamedSha !== null && sha256(exported) === streamedSha,
  };
  record("export", result);
  console.log(JSON.stringify(result, null, 2));
  if (!result.exportEqualsStored || !result.matchesStreamedTextSha) process.exitCode = 1;
}

const [cmd, arg] = process.argv.slice(2);
try {
  if (cmd === "selftest") await selftest();
  else if (cmd === "signin") await signin();
  else if (cmd === "check") await check();
  else if (cmd === "import") {
    const i = process.argv.indexOf("--name");
    console.log(JSON.stringify(importSource(arg, i >= 0 ? process.argv[i + 1] : "guidance"), null, 2));
  } else if (cmd === "request")
    console.log(createRequest(arg, process.argv.flatMap((a, i) => (a === "--select" && process.argv[i + 1] ? [process.argv[i + 1]] : []))));
  else if (cmd === "show") console.log(JSON.stringify(findRequest(loadStore(), arg), null, 2));
  else if (cmd === "export") exportLatest(arg);
  else if (cmd === "infer") {
    const i = process.argv.indexOf("--cut-after");
    await infer(arg, { cutAfter: i >= 0 ? Number(process.argv[i + 1]) : null, invalidAuth: process.argv.includes("--invalid-auth") });
  }
  else {
    console.error("usage: bun src/cli.ts selftest|import|request|signin|check|infer|export|show (see header)");
    process.exitCode = 2;
  }
} catch (e) {
  console.error(errorSummary(e));
  process.exitCode = 1;
}
