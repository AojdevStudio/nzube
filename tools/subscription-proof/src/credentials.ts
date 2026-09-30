// Credential rules for the direct SIWC route: identity binding on sign-in and scope checks on refresh.
import { SafeError } from "./diagnostics";
import { withFileLock } from "./lock";
import type { JsonValue } from "./json";

export const PLAN_SCOPE = "chatgpt.tokens.use.direct";

export type Credential = {
  client_id: string;
  subject: string;
  ext_agent_host_id: string;
  id_token: string;
  access_token: string;
  refresh_token: string | null;
  token_type: string;
  scopes: string[];
  expires_at: number; // unix seconds
  saved_at: string;
};

/** Hash of the validated ID-token `sub` bound to a registration (kept on disk instead of the raw sub). */
export const subjectBinding = (sub: string) => new Bun.CryptoHasher("sha256").update(sub).digest("hex");

/**
 * Checks a newly validated identity against the registration's existing binding before any
 * credential is replaced. `bound` is null only for a first-time registration.
 */
export function checkIdentityBinding(bound: string | null, validatedSub: string): void {
  if (bound !== null && bound !== subjectBinding(validatedSub))
    throw new SafeError("identity_mismatch", "validated identity does not match this registration's bound account; credentials left unchanged");
}

/**
 * Builds the replacement credential from a refresh-token response. Throws when the grant no
 * longer includes ChatGPT plan use, so the caller keeps the prior state and never infers.
 */
export function applyRefresh(prev: Credential, tok: { [k: string]: JsonValue }, nowSeconds: number, savedAt: string): Credential {
  const scopes = tok.scope ? String(tok.scope).split(" ").filter(Boolean) : prev.scopes;
  if (!scopes.includes(PLAN_SCOPE)) throw new SafeError("scope_missing", `refresh dropped ${PLAN_SCOPE}; prior credential kept, no inference`);
  return {
    ...prev,
    access_token: tok.access_token as string,
    refresh_token: (tok.refresh_token as string | undefined) ?? prev.refresh_token,
    scopes,
    expires_at: nowSeconds + Number(tok.expires_in ?? 0),
    saved_at: savedAt,
  };
}

/** True when an access token is within five minutes of expiry. */
export const refreshDue = (cred: Credential, nowSeconds: number) => cred.expires_at - 300 <= nowSeconds;

/**
 * Returns a usable credential, refreshing it at most once per session across processes. The
 * stored credential is re-read under the session lock, so a waiter uses the token another process
 * just rotated instead of replaying the old refresh token (profiles-and-sessions: serialize refreshes).
 */
export async function refreshSerialized(opts: {
  clientId: string;
  lockPath: string;
  store: { load(id: string): Promise<Credential | null>; save(id: string, c: Credential): Promise<void> };
  refresh: (cred: Credential) => Promise<{ [k: string]: JsonValue }>;
  nowSeconds: () => number;
}): Promise<Credential> {
  const current = await opts.store.load(opts.clientId);
  if (!current) throw new SafeError("auth", "no keyring credential for the saved registration; sign in again");
  if (!refreshDue(current, opts.nowSeconds())) return current;
  return withFileLock(opts.lockPath, async () => {
    const fresh = await opts.store.load(opts.clientId);
    if (!fresh) throw new SafeError("auth", "no keyring credential for the saved registration; sign in again");
    if (!refreshDue(fresh, opts.nowSeconds())) return fresh;
    if (!fresh.refresh_token) throw new SafeError("auth", "access token expired and no refresh token");
    // applyRefresh throws on loss of the plan scope before anything is saved.
    const next = applyRefresh(fresh, await opts.refresh(fresh), opts.nowSeconds(), new Date().toISOString());
    await opts.store.save(opts.clientId, next);
    return next;
  });
}

/**
 * Stores a freshly signed-in credential under the same per-session lock as refresh, so an
 * in-flight refresh finishes (and saves) first and can never overwrite the newer grant; any
 * later refresh re-reads this credential under the lock.
 */
export async function storeSignedInCredential(opts: {
  clientId: string;
  lockPath: string;
  store: { save(id: string, c: Credential): Promise<void> };
  cred: Credential;
}): Promise<void> {
  await withFileLock(opts.lockPath, () => opts.store.save(opts.clientId, opts.cred));
}
