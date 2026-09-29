# Compatibility and feasibility receipt

Run date: 2026-09-29, on one Linux desktop, with one real user consent. It covers two sets of runs:

- The first five generations ran on the pre-promotion proof code.
- One later generation ran on this package's CLI at commit `d962cb24237be5fa6be28cee223c43a6f4c5deb2`. See [Promoted CLI run](#promoted-cli-run).

This file records what the runs showed and makes no claim about platforms that were not tested. It contains no credentials, client ids, request ids, account details, host names, or addresses.

## Compatibility

| Question | Result | Evidence |
| --- | --- | --- |
| Can a user's existing ChatGPT plan power Nzube generation without a separately billed API key? | Yes on Linux desktop, through direct Sign in with ChatGPT | Real consent, `chatgpt.tokens.use.direct` granted, five completed generations below plus one through this package's CLI |
| Billing route | Inferred as ChatGPT plan: plan-scoped OAuth bearer on `POST /v1/responses`, no API key present, no fallback path | No usage-meter reading was taken |
| Needs a Nzube cloud account or hosted backend | No | Public PKCE client, loopback callback, local store |
| Needs Codex CLI or another companion | No, for this route | The CLI talks to OpenAI directly |
| Browser on another machine | Works through `ssh -N -L 1455:127.0.0.1:1455` plus a short `/start` redirect | The callback stayed on 127.0.0.1 |
| Credential storage | OS keyring (Linux Secret Service), one record per issued client | No token on disk or in logs |
| Token refresh | Worked once, live, through the package's serialized refresh | Promoted CLI run below. Sign-out revocation is not implemented. |
| macOS, Windows desktop | Unverified | No keyring implementation or run |
| iOS, Android | Unverified | OpenAI documents only an HTTP loopback redirect; a mobile redirect is undocumented |
| Codex app-server route (separate) | Unproven | Its login was not approved during this run |

## Feasibility results

Model: `gpt-5.6-luna`, chosen from the account's `/v1/models` list and reported as served in `response.completed`. Every generation used `store:false`, `stream:true`, and no tools, and its output items were only `reasoning` and `message`.

| Case | Guidance | Outcome | Output sha256 | Chars |
| --- | --- | --- | --- | --- |
| Bug fix, unconfirmed diagnosis | v1 | completed | `7a4ad06801c4fa4b7a68d3550c1cb5f6ee4c88f5ebfbd0046cbc8b277ad69d3c` | 2,515 |
| Read-only investigation | v1 | completed | `98b456d950aadfcadd343eff28e01e073915ad0bfb13e452d0ac6f687121a2a0` | 2,573 |
| Same bug fix, no guidance | none | completed | `eb89a31201d451cf2b6f1d8289beb83685293e28b8bcb7a77d17eaf3b0f28521` | 2,927 |
| Refinement of the first brief | v1 | completed | `828dce3f195db249d77734ed51ca7202aa5d0dd98d64815fd3773abaa6495cee` | 2,565 |
| Read-only investigation, regenerated | v2 | completed | `8f4ce90bd84c7fe6b2a72efc4b3a5b27acfdc5ee399a71a25da2dd277199940e` | 2,559 |
| Ctrl-C after the first streamed delta | v1 | cancelled, exit 130 | 17-char partial kept separately | |
| Stream cut after 8 deltas | v1 | failed `stream_incomplete`, exit 1 | 42-char partial kept separately | |
| Invalid bearer | v1 | failed, 401, exit 1, no fallback | | |

- **Guidance hashes:** v1 is `efd995ad89450a65924dd4c1dec02e1c8da57afe13e2458c605de161e89dfa8b`. v2 is `b0d974e88839ae7f17355fef9e928d1e8076726181b05bd708c062b5131612c9` and is `fixtures/guidance.md`.
- **Request hashes:** `fixtures/request.md` is `b59585b0…` and `fixtures/request-readonly.md` is `cf1a7366…`, identical to the requests used in the run.
- **Export:** the first brief was exported as 2,515 bytes. The export is byte-equal to the stored output, and its sha256 equals the hash of the streamed text. After the cancellation and stream-cut runs, the stored output was unchanged and a second export was identical.
- **Draft safety:** every attempt kept the original request text unchanged (hash-checked).

## Promoted CLI run

This run tested the package code itself, after the lifecycle, SSE, concurrency, and lock changes. It used `tools/subscription-proof` at commit `d962cb24237be5fa6be28cee223c43a6f4c5deb2`, which an independent review had passed on Linux with glibc. It ran on 2026-09-29 at 23:45–23:46Z. No new consent was given: it reused the consent and client registration from the first runs, and the credential stayed in the OS keyring. It used no API key and no other route.

| Step | Result |
| --- | --- |
| `check` | The stored access token had expired, so the CLI refreshed it live under the per-session refresh lock. The refreshed credential kept the plan scope. `GET /v1/models` returned 200 and listed `gpt-5.6-luna`. |
| `infer` | Read-only investigation request (`fixtures/request-readonly.md`) with guidance v2 (`fixtures/guidance.md`, `b0d974e8…`). `POST /v1/responses` with `store:false`, `stream:true`, no tools. `gpt-5.6-luna` was selected and served, the stream ended in `response.completed` with `reasoning` and `message` items only, and usage was 535 input and 998 output tokens. |
| `export` | 3,236 bytes, sha256 `1521adb6d73f67af9a489c625927ebb45d501663af49b6d86af6eeef884f3425`. The export is byte-equal to the stored output (full-byte comparison), and its hash equals the hash of the streamed text. |

- **The brief:** it keeps "Findings only" and the read-only constraint, limits proof to existing tests or non-mutating reproduction, says not to create or modify tests or source, and ends with a stop point.
- **Billing:** as for the first runs, it is inferred from the route and the plan scope. No usage-meter reading was taken.
- **Scope:** this is a single request on one model. It shows that the promoted code completes and persists a real generation; it is not a quality evaluation.

## Reviewed quality

This is a reading of the outputs, not a score.

**Guidance vs no guidance, same request:**

- Both briefs keep the suggested pagination cause unconfirmed.
- With guidance, the brief separates verified, reported, hypothesized, and unverified material, copies both user constraints word for word, requires a failing test first, and ends with an explicit stop point at the requested endpoint.
- Without guidance, the brief paraphrases the constraints, has no stop point, and adds more boundary tests.
- Neither invented paths, commands, or issue numbers.

**Refinement:** the full revised brief came back. It differs from the original in exactly one line, the requested extra test case.

**Read-only investigation:**

- With v1, the brief respected "Findings only" and the read-only constraint, but still asked for "a failing test or equivalent reproducible test case". The bug-fix proof rule leaked into an investigation.
- v2 narrowed the rule to fit what the request authorizes. The regenerated brief uses existing tests or a non-mutating reproduction and says "Do not create or modify tests".
- The v2 brief is slightly stricter than asked ("Do not propose or implement a fix"), and it applies the `TBD (discover in repo)` rule to paths the findings should report once discovered.

## Not shown

- sign-out revocation, and token refresh beyond the single live refresh above
- a ChatGPT usage-meter reading
- macOS, Windows, iOS, and Android
- adversarial or broader prompt-quality evaluation
- grounding against a real repository (the requests are synthetic)
