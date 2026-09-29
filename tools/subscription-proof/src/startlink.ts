// Reusable, non-secret start link for a remote browser: GET /start redirects to the authorize URL
// of the currently pending `cli.ts signin`. The OAuth callback itself stays on 127.0.0.1:1455 and is
// reached from the remote browser only through `ssh -N -L 1455:127.0.0.1:1455 ...`.
// Binds to one given address (the tailnet IP), never 0.0.0.0. Logs method, path and status only.
// Usage: bun startlink.ts <bind-address> <port> [lifetime-minutes]
import { existsSync, readFileSync } from "node:fs";
import { connect } from "node:net";
import { join } from "node:path";
import { dataRoot } from "./store";

const [hostname, portArg, lifetimeArg] = process.argv.slice(2);
if (!hostname || hostname === "0.0.0.0" || hostname === "::") throw new Error("bind to one specific address");
const port = Number(portArg);
const actionFile = join(dataRoot, "signin-action.md");
const LISTENER_MINUTES = 30; // cli.ts signin waits 30 minutes for its callback

/** The pending authorize URL and its expiry, or null when no sign-in is pending. */
function pending(): { url: string; expiresAt: Date } | null {
  if (!existsSync(actionFile)) return null;
  const text = readFileSync(actionFile, "utf8");
  if (text.includes("\nResult:")) return null; // that sign-in already finished
  const url = text.match(/https:\/\/auth\.openai\.com\/api\/accounts\/authorize\?\S+/)?.[0];
  const started = text.match(/^Started: (\S+)/m)?.[1];
  if (!url || !started) return null;
  const expiresAt = new Date(new Date(started).getTime() + LISTENER_MINUTES * 60_000);
  return expiresAt > new Date() ? { url, expiresAt } : null;
}

/** True when the loopback callback listener accepts connections. */
function callbackListening(): Promise<boolean> {
  return new Promise((resolve) => {
    const s = connect({ host: "127.0.0.1", port: 1455 });
    s.once("connect", () => (s.destroy(), resolve(true)));
    s.once("error", () => resolve(false));
  });
}

const server = Bun.serve({
  hostname,
  port,
  async fetch(req) {
    const path = new URL(req.url).pathname;
    let res: Response;
    const p = pending();
    if (path === "/start") {
      res = p && (await callbackListening())
        ? new Response(null, { status: 302, headers: { location: p.url, "cache-control": "no-store", "referrer-policy": "no-referrer" } })
        : new Response("No Nzube sign-in is pending. Ask the coordinator to start a new one.\n", { status: 410 });
    } else if (path === "/status") {
      res = Response.json({ pending: Boolean(p), expiresAt: p?.expiresAt.toISOString() ?? null, callbackListening: await callbackListening() });
    } else res = new Response("not found\n", { status: 404 });
    console.log(`${new Date().toISOString()} ${req.method} ${path} ${res.status}`);
    return res;
  },
});
console.log(`READY http://${server.hostname}:${server.port}/start`);
setTimeout(() => (server.stop(true), process.exit(0)), Number(lifetimeArg ?? 240) * 60_000);
