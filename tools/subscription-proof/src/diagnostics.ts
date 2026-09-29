// Persisted diagnostics: only fixed local categories, allowlisted codes and numbers.
// External text (stderr, provider messages, server-supplied codes or types) stays in memory and
// never reaches evidence, the store, or stdout that chains redirect into files.

type Json = string | number | boolean | null | Json[] | { [k: string]: Json };

export const failureCategories = [
  "gate",
  "timeout",
  "http_error",
  "network_error",
  "auth",
  "identity_mismatch",
  "scope_missing",
  "stream_incomplete",
  "unsafe_events",
  "storage_error",
  "unexpected_error",
] as const;
export type FailureCategory = (typeof failureCategories)[number];

/** An error whose message is a fixed local string, safe to persist as `reason`. */
export class SafeError extends Error {
  constructor(
    readonly category: FailureCategory,
    reason: string,
  ) {
    super(reason);
  }
}

/** Documented SIWC Responses and OAuth error codes (errors-and-recovery.md, RFC 6749). */
const knownProviderCodes = new Set([
  "subscription_sharing_user_not_eligible",
  "subscription_sharing_usage_limit_exceeded",
  "subscription_sharing_usage_unavailable",
  "subscription_sharing_unsupported_capability",
  "subscription_sharing_route_not_supported",
  "subscription_sharing_invalid_user",
  "subscription_sharing_user_unavailable",
  "chatpass_v2_scope_not_authorized",
  "chatpass_v2_invalid_authorization_context",
  "access_denied",
  "invalid_request",
  "invalid_client",
  "invalid_grant",
  "invalid_scope",
  "unauthorized_client",
  "unsupported_grant_type",
  "unsupported_response_type",
  "server_error",
  "temporarily_unavailable",
  "invalid_refresh_token",
  "token_expired",
  "refresh_token_expired",
  "refresh_token_invalidated",
  "refresh_token_reused",
]);

/** Keeps a protocol identifier (method or event name) only when it has identifier shape. */
export function identifierShape(value: unknown, fallback = "other"): string {
  return typeof value === "string" && /^[a-z][a-zA-Z0-9_./]{0,63}$/.test(value) ? value : fallback;
}

/** Re-keys a count map so only identifier-shaped keys survive; the rest fold into `other`. */
export function shapedCounts(counts: Record<string, number>): Record<string, number> {
  const out: Record<string, number> = {};
  for (const [k, n] of Object.entries(counts)) {
    const key = identifierShape(k);
    out[key] = (out[key] ?? 0) + n;
  }
  return out;
}

/** Returns `value` when it is in `allowed`, else `fallback`. */
export function allowlisted(value: unknown, allowed: ReadonlySet<string>, fallback: string): string {
  return typeof value === "string" && allowed.has(value) ? value : fallback;
}

/** A provider/OAuth error code as it may be persisted; anything undocumented is replaced. */
export function providerCode(value: unknown): string | null {
  if (value === undefined || value === null) return null;
  return allowlisted(value, knownProviderCodes, "unknown_provider_error");
}

/** Persistable summary of a thrown error: category, plus a fixed reason. */
export function errorDiagnostic(e: unknown): { [k: string]: Json } {
  if (e instanceof SafeError) return { category: e.category, reason: e.message };
  if (e instanceof TypeError) return { category: "network_error" };
  return { category: "unexpected_error" };
}

/** One-line persistable form of `errorDiagnostic`, for attempt records and console output. */
export function errorSummary(e: unknown): string {
  const d = errorDiagnostic(e);
  return [d.category, d.reason].filter((x) => x !== undefined).join(": ");
}
