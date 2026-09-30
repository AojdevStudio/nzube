// Local mock of the OpenAI endpoints the CLI uses, for tests only. Serves OIDC discovery, JWKS, a
// token endpoint that signs a real RS256 ID token, /models, and a streamed /responses whose SSE
// line endings and chunk boundaries the test chooses.

export type Framing = "lf" | "crlf" | "cr";
export type StreamMode = "complete" | "failed" | "cut";

const b64url = (data: string | Uint8Array) => Buffer.from(data).toString("base64url");

export async function startMockOpenAI() {
  const { privateKey, publicKey } = await crypto.subtle.generateKey(
    { name: "RSASSA-PKCS1-v1_5", modulusLength: 2048, publicExponent: new Uint8Array([1, 0, 1]), hash: "SHA-256" },
    true,
    ["sign", "verify"],
  );
  const jwk = { ...(await crypto.subtle.exportKey("jwk", publicKey)), kid: "test-key", alg: "RS256", use: "sig" };
  const state = {
    nonce: "",
    grantedScope: "openid profile email offline_access resource.invoke chatgpt.tokens.use.direct",
    framing: "lf" as Framing,
    mode: "complete" as StreamMode,
    text: "Outcome: mock brief.\nLine two.",
  };

  async function idToken(clientId: string) {
    const header = b64url(JSON.stringify({ alg: "RS256", kid: "test-key", typ: "JWT" }));
    const now = Math.floor(Date.now() / 1000);
    const payload = b64url(JSON.stringify({ iss: base, aud: clientId, sub: "test-subject", nonce: state.nonce, iat: now, exp: now + 600 }));
    const sig = await crypto.subtle.sign("RSASSA-PKCS1-v1_5", privateKey, new TextEncoder().encode(`${header}.${payload}`));
    return `${header}.${payload}.${b64url(new Uint8Array(sig))}`;
  }

  function sse(): ReadableStream<Uint8Array> {
    const eol = state.framing === "crlf" ? "\r\n" : state.framing === "cr" ? "\r" : "\n";
    const events: object[] = [
      { type: "response.created" },
      { type: "response.output_item.added", item: { type: "message" } },
      ...[...state.text].map((ch) => ({ type: "response.output_text.delta", delta: ch })),
    ];
    if (state.mode === "complete") events.push({ type: "response.completed", response: { model: "gpt-5.6-luna", usage: { output_tokens: 5 } } });
    if (state.mode === "failed") events.push({ type: "response.failed", response: { error: { code: "server_error" } } });
    const wire = events.map((e) => `event: ${(e as { type: string }).type}${eol}data: ${JSON.stringify(e)}${eol}${eol}`).join("");
    // Odd chunk sizes so line endings (including CR|LF pairs) straddle chunk boundaries.
    const bytes = new TextEncoder().encode(wire);
    return new ReadableStream({
      start(c) {
        for (let i = 0; i < bytes.length; i += 7) c.enqueue(bytes.slice(i, i + 7));
        c.close();
      },
    });
  }

  let base = "";
  const server = Bun.serve({
    hostname: "127.0.0.1",
    port: 0,
    async fetch(req): Promise<Response> {
      const url = new URL(req.url);
      switch (url.pathname) {
        case "/.well-known/openid-configuration":
          return Response.json({
            issuer: base,
            authorization_endpoint: `${base}/api/accounts/authorize`,
            token_endpoint: `${base}/api/accounts/oauth/token`,
            jwks_uri: `${base}/jwks`,
            revocation_endpoint: `${base}/api/accounts/oauth/revoke`,
          });
        case "/jwks":
          return Response.json({ keys: [jwk] });
        case "/api/accounts/oauth/token": {
          const form = new URLSearchParams(await req.text());
          const clientId = form.get("client_id") ?? "";
          return Response.json({
            access_token: "mock-access",
            refresh_token: "mock-refresh",
            id_token: await idToken(clientId),
            token_type: "Bearer",
            expires_in: 3600,
            scope: state.grantedScope,
          });
        }
        case "/v1/models":
          return Response.json({ models: [{ slug: "gpt-5.6-luna", visibility: "list" }] });
        case "/v1/responses":
          return new Response(sse(), { headers: { "content-type": "text/event-stream" } });
        default:
          return new Response("not found", { status: 404 });
      }
    },
  });
  base = `http://127.0.0.1:${server.port}`;
  return { base, apiBase: `${base}/v1`, state, stop: () => server.stop(true) };
}

/** A currently free loopback port. */
export function freePort(): number {
  const s = Bun.serve({ hostname: "127.0.0.1", port: 0, fetch: () => new Response() });
  const port = s.port;
  s.stop(true);
  if (port === undefined) throw new Error("no port");
  return port;
}
