# Subscription proof: direct Sign in with ChatGPT

A small Bun and TypeScript CLI that proves Nzube can generate an execution brief using the user's ChatGPT plan through OpenAI's documented open-source Sign in with ChatGPT flow. It has no API key and no billing fallback. It is a feasibility tool, not the product: there is no UI and no framework. The results of the run that validated it are in [RECEIPT.md](RECEIPT.md).

## What it does

- **Sign-in (`signin`):** registers a public OAuth client dynamically (`client_id=dynamic_agent_client`) with PKCE S256, `state`, and `nonce`. It waits for the browser callback on `http://127.0.0.1:1455/auth/callback` and exchanges the code without a client secret.
  - It validates the ID token: RS256 signature against OpenAI's JWKS, issuer, audience, expiry, and nonce.
  - It requires the `chatgpt.tokens.use.direct` scope.
  - It stores the credential in the OS keyring.
- **Model check (`check`):** lists the models the signed-in account can use (`GET /v1/models`).
- **Library (`import`, `request`):** keeps a small local library.
  - Guidance files are content-addressed and versioned.
  - Requests are persisted with an explicit guidance selection.
- **Generation (`infer`):** assembles the selected guidance and the request, then makes one streamed `POST /v1/responses` call with `store:false`, `stream:true`, and no tools.
  - Only a completed stream with no tool items becomes the attempt's complete output.
  - A failed, cut, or cancelled stream keeps its text as a separate partial file, and the request is never lost.
- **Export (`export`):** writes the latest complete output and proves the exported bytes equal the stored output and the hash of the streamed text.
- **Remote browser (`src/startlink.ts`):** an optional short `/start` redirect bound to one address you choose. Use it when the browser runs on another machine that reaches this one through `ssh -N -L 1455:127.0.0.1:1455 <user>@<host>`.

## Requirements

- Bun 1.x.
- Linux with a Secret Service keyring and `secret-tool` (libsecret). `src/keyring.ts` defines a `CredentialStore` interface; macOS Keychain and Windows Credential Manager implementations do not exist yet.
- A browser that can reach `127.0.0.1:1455` on the machine running the CLI, directly or through an SSH local forward.
- A ChatGPT account whose workspace allows plan use by open-source apps.

## Use

```sh
cd tools/subscription-proof
bun install
bun src/cli.ts selftest                                   # no sign-in; fetches OIDC discovery (network), writes and clears a dummy keyring entry, checks the PKCE vector
bun src/cli.ts import fixtures/guidance.md --name brief-guidance
bun src/cli.ts request fixtures/request.md --select brief-guidance@v1
bun src/cli.ts signin                                     # open the printed URL (see <data>/signin-action.md), consent
bun src/cli.ts check
bun src/cli.ts infer req-1
bun src/cli.ts export req-1
bun src/cli.ts infer req-1 --cut-after 8                  # real stream cut: failure plus kept partial
bun src/cli.ts infer req-1 --invalid-auth                 # real 401: failure, no fallback
```

- **Data location:** `$NZUBE_PROOF_DATA`, else `$XDG_STATE_HOME/nzube-subscription-proof`, else `~/.local/state/nzube-subscription-proof`. It holds the store, per-attempt outputs and partials, exports, non-secret registration metadata, and `evidence.json`.
- **Evidence contents:** `evidence.json` never holds tokens or response bodies.
  - **Errors:** stored as a fixed category plus a fixed local reason, the HTTP status, a documented error code (anything else becomes `unknown_provider_error`), and the body length. Provider messages, descriptions and `detail` text are never written.
  - **Other server-supplied values:** some are stored without content validation:
    - the OIDC discovery document (`selftest`)
    - model slugs from `/v1/models` (the listed models and the selected model in `check` and `infer`; the served model only when it matches a listed slug)
    - `x-request-id` values, kept only when they have an opaque token shape
  - **Server values with limits applied:**
    - Granted scopes: only known scopes are kept, plus a count of the others.
    - Token-response field names and output item types: checked against fixed allowlists.
    - Stream event names: counted only when identifier-shaped.
  - **Local values:** source ids, attempt ids, input and output hashes, sizes, and token usage counts.
- **Exit codes:** every command exits nonzero on failure.
  - `signin` exits 0 only when the grant includes ChatGPT plan use, and it exits as soon as the callback is handled.
  - Pressing Ctrl-C during `infer` exits 130 and keeps the partial.
- **Concurrency:** overlapping runs coordinate through lock files.
  - Store updates and attempt ids go through `<data>/store/.lock`.
  - Evidence appends go through `<data>/evidence.json.lock`.
  - Token refresh is serialized per session (`<data>/state/refresh.lock`), and the stored credential is re-read under the lock, as OpenAI requires for rotating refresh tokens.
  - A lock whose owner process no longer exists is taken over.
  - Tests check 8 overlapping failed runs and 40 overlapping successful runs, including one evidence entry per generated attempt.
  - Takeover of a crashed owner's lock is not covered by an atomic compare-and-remove. Two waiters could in principle both clear the same stale lock. A 960-process probe did not observe it.
- **Streams:** they are parsed per the SSE format, so LF, CRLF, or CR line endings split anywhere across chunks all work.

## Tests

```sh
bun run check    # bun test (40 checks) and tsc --noEmit
```

The tests use no real network, credentials, or OS keyring.

**Full CLI runs** against a local mock of the OpenAI endpoints (`test/mock-openai.ts`, including a real RS256-signed ID token) cover:

- sign-in callbacks that succeed, arrive with the wrong state, report denied consent, or lack the plan scope, each with its exit status and a prompt exit
- streamed generations with LF, CRLF, and CR framing, plus failed and cut streams
- 8 overlapping failed runs that keep every attempt with a unique id, and 40 overlapping successful runs that keep one evidence entry per generated attempt

**Direct unit tests** cover:

- commit rules for complete, failed, unsafe, and interrupted streams
- identity binding before a credential is replaced
- refresh rejection when the plan scope is lost
- non-2xx handling
- allowlisted error diagnostics, with fixture text containing personal-looking strings that must not survive serialization
- per-attempt history
- CLI exit codes
- single-flight token refresh
- SSE framing at every chunk size

**Test-only overrides.** `NZUBE_PROOF_ISSUER`, `NZUBE_PROOF_API_BASE`, and `NZUBE_PROOF_CALLBACK_PORT` point the CLI at mocks. The two URL overrides are honored only for `127.0.0.1` or `localhost`. `NZUBE_PROOF_KEYRING=memory` keeps credentials in process memory, never on disk. Real use sets none of them.

## Fixtures

The files in `fixtures/` are original synthetic text written for this proof. They are not copied from any private or third-party source.

- `guidance.md`: version 2 of the sample brief-writing guidance (sha256 `b0d974e8…`, the "v2" in the receipt). Its proof rule fits what the request authorizes. A fresh store numbers it `brief-guidance@v1`, because store versions count imports.
- `request.md`: a bug fix with an unconfirmed diagnosis.
- `request-readonly.md`: a read-only investigation.

## Limitations

- Only Linux desktop was exercised. Mobile sign-in is unverified: OpenAI documents only an HTTP loopback redirect.
- Token refresh against the live endpoint and sign-out revocation have not run. Refresh rules are covered by offline tests, and revocation is not implemented.
- Billing to the ChatGPT plan is inferred from the documented route and the granted scope. No usage-meter reading was taken.
- Prompt quality was judged on five outputs from one model. That is not an evaluation.
- OpenAI marks this flow as a preview. See its preview limitations before relying on it.

Docs: https://developers.openai.com/siwc/token-sharing-open-source/ (sign-in, profiles-and-sessions, models-and-inference, errors-and-recovery, preview-limitations).
