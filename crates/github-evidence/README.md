# github-evidence

Read-only GitHub retrieval for Nzube. It fetches the repository evidence that grounds a brief: files pinned to a commit, the repository's agent instructions file, issues and their comments, pull requests with their files and diff, and commit history. It never writes to GitHub.

This crate is a feasibility and core-contract unit. It does not choose the application framework, and it has no UI.

## Status

| Capability | State |
| --- | --- |
| Endpoint allowlist, input parsing, error classification, page and byte caps, pull request revision binding, redaction | Covered by 26 synthetic contract tests (`tests/contract.rs`) |
| Anonymous reads of a public repository, including revision-bound files and diff | Exercised live against the public REST API during development |
| GitHub App device flow, secretless refresh, and auth failure, expiry, and disconnect paths | Covered by synthetic tests only |
| Secure-storage and disconnect lifecycle (`connection`) | Covered by 9 synthetic tests (`tests/connection.rs`) with an in-memory stand-in for platform storage. No real Keychain, Keystore, Secret Service, or Credential Manager adapter exists yet |
| **Product authentication with a real GitHub App user token** | **Unverified.** No Nzube GitHub App is registered, so no client ID exists. See [docs/github-app-setup.md](docs/github-app-setup.md) |
| Private repositories, revoked grants, uninstalled repositories, SSO-protected organizations | Unverified live |
| Mobile targets (iOS, Android) | Not built. The TLS stack (`rustls` with `aws-lc-rs`) compiles C code and needs a cross-compile check |

## What it can request

`EvidenceRequest` is the public request type. `Endpoint` is the only thing that builds a URL, and every endpoint is a GET on `https://api.github.com` with `X-GitHub-Api-Version: 2026-03-10`.

| Request | REST call | App permission |
| --- | --- | --- |
| `ResolveRevision`, `History` | `GET /repos/{owner}/{repo}/commits?sha=&path=&per_page=` | Contents: read |
| `File`, `InstructionsFile` | `GET /repos/{owner}/{repo}/contents/{path}?ref={commit}` | Contents: read |
| `Issue` | `GET /repos/{owner}/{repo}/issues/{number}` | Issues: read |
| `IssueComments` | `GET /repos/{owner}/{repo}/issues/{number}/comments` | Issues: read or Pull requests: read |
| `PullRequest` | `GET /repos/{owner}/{repo}/pulls/{number}` | Pull requests: read |
| `PullRequestFiles` | `GET /repos/{owner}/{repo}/pulls/{number}/files` | Pull requests: read |
| `PullRequestDiff` | `GET /repos/{owner}/{repo}/pulls/{number}` with `Accept: application/vnd.github.diff` | Pull requests: read |

`InstructionsFile` checks `AGENTS.md`, `CLAUDE.md`, and `.github/copilot-instructions.md`, in that order, at the given commit.

`PullRequestFiles` and `PullRequestDiff` take the `PullRevisions` (base and head commits) from an earlier `PullRequest` read, and the evidence they return carries those revisions. After reading, the client reads the pull request again. If either the base or the head has moved, the result is `Partial(RevisionsChanged)` with the expected and observed SHAs. A head moves on a push or force-push. A base moves when the base branch advances or the pull request is retargeted. If the second read fails for any reason, including 401, 403, 404, or a rate limit, the result is `Partial(RevisionsUnverified)`. `Complete` means the base and head matched `expected` when the pull request was read before and after the content. It is not an atomic snapshot.

The files and diff endpoints are mutable: they describe the pull request as it is now, not a fixed commit pair. The re-read catches any change still visible when it runs, but not every change. A base or head that moves away and back to the same SHA between the two reads (ABA) is not detected, so content read at the intermediate commit can still be marked `Complete`. Every result carries the expected revisions, and every receipt records the resource, the time the response arrived, and the body's SHA-256. Comparing two fixed SHAs (`GET /repos/{owner}/{repo}/compare/{base}...{head}`) would give an immutable diff. It is not in the allowlist yet, and its limits and its equivalence to the pull request diff have not been checked.

## Boundaries

- **Input.** `RepoRef` accepts `owner/repo` or a `https://github.com/owner/repo` URL. It rejects other hosts, ports, credentials in URLs, and extra path segments. `RepoPath` rejects absolute paths, `.` and `..` segments, empty segments, backslashes, and control characters. Each path segment is percent-encoded, so `?`, `#`, and `%` stay inside the path.
- **Pagination.** From GitHub's `Link: rel="next"` header, the client reads only the page number, and only when it is the next page in sequence. It rebuilds the URL from its own endpoint. GitHub's next links point at `/repositories/{id}/...`, and a URL taken from a response is never requested.
- **Redirects.** Redirects are never followed, so a credential cannot be replayed to another host. A 3xx response becomes `FetchError::Moved`.
- **Limits.** `Limits` caps items per page, page count, bytes per response, and total bytes per `fetch`. The total budget is checked before every read, and each read is capped at the smaller of the per-response cap and the remaining budget, so the last page cannot overrun it. Any limit set to zero is rejected with `InvalidLimits` before a request is made. When a cap is hit, or an error arrives after the first page, the result is `Completeness::Partial` with a reason, and the items already retrieved are kept.
- **File list ceiling.** GitHub lists at most 3000 files for a pull request. A file list shorter than the pull request's `changed_files` is reported as `Partial(FilesBelowChangedCount)`.
- **Mutable content.** Issues, comments, and pull request metadata can change between reads. A multi-page list is not an atomic snapshot. Each receipt records when its response arrived and the SHA-256 of its body.
- **Errors.** `FetchError` separates `InvalidCredentials` (401), `PermissionDenied` (403, with SSO-required and not-granted-to-token reasons), `RateLimited` (primary or secondary, on 403 or 429, following GitHub's documented headers), `Unavailable` (404: missing *or* not visible, never proof of nonexistence), `Moved`, `Gone`, `Transport`, `Malformed`, `ResponseTooLarge`, `ByteBudgetExhausted`, `InvalidLimits`, and `PaginationUnrecognized`. Error text never contains a token, a URL from a response, or a response body.
- **Secrets.** `SecretToken` has a redacted `Debug` and no `Display`, so formatting an error, request, receipt, or evidence record never prints it. The raw value is readable through `SecretToken::expose_secret`, which exists for platform secure-storage adapters. Code that calls it must keep the value out of logs, telemetry, exports, and prompts. Inside this crate, only the HTTP transport reads it, to set the Authorization header or an OAuth form field.
- **Provenance.** Every `Fetched` names the repository it read. Every call yields a `Receipt` with method, host, path and query, status, GitHub request id, rate-limit headers, body size, body SHA-256, and the time the response arrived. A receipt never includes a credential or a body.
- **Auth is separate.** `auth::DeviceFlow` posts only to `github.com/login/device/code` and `github.com/login/oauth/access_token`. `EvidenceClient` never sends POST and never contacts github.com. `Credential` must be chosen explicitly. The crate never reads ambient credentials such as a `gh` login or environment tokens.

## Connection lifecycle

`connection::Connection` keeps a user grant in a `SecretStore` that the application implements over platform secure storage. There is no file, environment, or ambient-login fallback.

- **Connect.** A new grant becomes usable only after it has been saved.
- **Expiry.** An access token within 60 seconds of expiry is refreshed without a client secret. The rotated grant is saved before the new token is returned. If that save fails, the new token is not returned, and the user must sign in again, because GitHub has already invalidated the old refresh token.
- **Rejected or expired refresh.** A missing, expired, or rejected refresh token deletes the stored grant and returns `ReauthRequired`.
- **Concurrent refresh.** A refresh token works once. When two callers refresh the same expired grant, the loser is refused. It deletes the stored grant only through `SecretStore::delete_if_refresh`, which the store must perform as one atomic compare-and-delete, and only while the grant still holds the refresh token it used. If another caller already rotated the grant, the loser returns the rotated access token instead.
- **Retryable failures.** Only an explicit refusal deletes the stored grant: an OAuth error response, or a 400 or 401 from the token endpoint. Throttling (429), other statuses, transport failures, and malformed responses keep the grant and return `Refresh(error)` for a retry.
- **Disconnect.** `disconnect` deletes the local tokens and reports `remote_grant_revoked: false` with `REVOKE_URL`, because a secretless client cannot revoke the grant on GitHub.
- **Storage adapters.** `SecretToken::expose_secret` exists only so storage adapters can write the value. Never log or display it.

## Client ID injection

The crate contains no client ID. The application reads its registered GitHub App's public client ID from its own configuration and passes it to `ClientId::parse`. The device flow sends only that ID. The token request and refresh never send a client secret. GitHub documents that a refresh does not require the secret when the user token came from the device flow. A granted token must start with `ghu_`, which is the documented GitHub App user-token prefix. Anything else, such as an OAuth App `gho_` token, is rejected.

The example takes the ID from an environment variable:

```sh
NZUBE_GITHUB_APP_CLIENT_ID=<client id> \
  cargo run -p github-evidence --example device_login -- <owner/repo> <branch> [issue] [pull]
```

## Develop

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

The tests use a fixture transport with original synthetic payloads. They make no network calls.
