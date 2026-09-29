// HTTP outcome handling for the direct SIWC route. Persisted diagnostics carry status, request id
// and an allowlisted code only; body text and server-supplied code/type strings are dropped.
import { providerCode } from "./diagnostics";
import type { JsonValue } from "./json";

type Obj = { [k: string]: JsonValue };

export type HttpFailure = { status: number; requestId: string | null; code: string | null; bodyChars: number };

function parse(bodyText: string): Obj {
  try {
    const v = JSON.parse(bodyText) as JsonValue;
    return v && typeof v === "object" && !Array.isArray(v) ? v : {};
  } catch {
    return {};
  }
}

/** Summarizes a non-2xx response for evidence: Responses `error.code` or OAuth `error`, allowlisted. */
export function httpFailure(status: number, bodyText: string, requestId: string | null): HttpFailure {
  const err = parse(bodyText).error;
  const nested = err && typeof err === "object" && !Array.isArray(err) ? err : null;
  const raw = nested ? nested.code : err;
  return { status, requestId: requestIdShape(requestId), code: providerCode(raw), bodyChars: bodyText.length };
}

/** Request ids are kept only in their opaque token shape. */
function requestIdShape(id: string | null): string | null {
  return id && /^[A-Za-z0-9_-]{1,128}$/.test(id) ? id : null;
}

export type ModelsResult = { ok: true; listed: string[] } | { ok: false; failure: HttpFailure };

/** Interprets a GET /v1/models response; any non-2xx is a failure, never an empty list. */
export function modelsFromResponse(status: number, bodyText: string, requestId: string | null): ModelsResult {
  if (status < 200 || status > 299) return { ok: false, failure: httpFailure(status, bodyText, requestId) };
  const models = parse(bodyText).models;
  const list = Array.isArray(models) ? (models as Obj[]) : [];
  return { ok: true, listed: list.filter((m) => m.visibility === "list").map((m) => String(m.slug)) };
}
