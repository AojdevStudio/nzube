// Provider endpoints. Defaults are OpenAI's documented values. Overrides exist so tests can run the
// real CLI against local mock servers, and are honored only for loopback URLs so a stray
// environment variable can never send credentials to another host.

const DEFAULT_ISSUER = "https://auth.openai.com";
const DEFAULT_API_BASE = "https://api.openai.com/v1";

function loopbackOverride(name: string, fallback: string): string {
  const value = process.env[name];
  if (!value) return fallback;
  const host = new URL(value).hostname;
  if (host !== "127.0.0.1" && host !== "localhost") throw new Error(`${name} must be a loopback URL`);
  return value.replace(/\/$/, "");
}

/** OAuth issuer; discovery, authorize, token and JWKS URLs are derived from it. */
export const ISSUER = loopbackOverride("NZUBE_PROOF_ISSUER", DEFAULT_ISSUER);
/** Base URL for `/models` and `/responses` requests. */
export const API_BASE = loopbackOverride("NZUBE_PROOF_API_BASE", DEFAULT_API_BASE);
/** OAuth `resource` parameter; always the documented audience. */
export const RESOURCE = DEFAULT_API_BASE;
/** Loopback callback port. The docs allow only the port to vary between sign-ins. */
export const CALLBACK_PORT = Number(process.env.NZUBE_PROOF_CALLBACK_PORT ?? 1455);
