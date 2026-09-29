// Credential rules for the direct SIWC route: identity binding on sign-in and scope checks on refresh.
import { SafeError } from "./diagnostics";
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
