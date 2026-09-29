# github-evidence

Read-only GitHub retrieval for Nzube. It fetches the repository evidence that grounds a brief: files pinned to a commit, the repository's agent instructions file, issues and their comments, pull requests with their files and diff, and commit history. It never writes to GitHub.

This crate is a feasibility and core-contract unit. It does not choose the application framework, and it has no UI.

## Status

| Capability | State |
| --- | --- |
| Endpoint allowlist, input parsing, error classification, page and byte caps, pull request revision binding, redaction | Covered by 26 synthetic contract tests (`tests/contract.rs`) |
| Anonymous reads of a public repository, including revision-bound files and diff | Exercised live against the public REST API during development |
| GitHub App device flow, secretless refresh, and auth failure, expiry, and disconnect paths | Covered by synthetic tests only |
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

`PullRequestFiles` and `PullRequestDiff` take the `PullRevisions` (base and head commits) from an earlier `PullRequest` read, and the evidence they return carries those revisions. After reading, the client reads the pull request again. If either the base or the head has moved, the result is `Partial(RevisionsChanged)` with the expected and observed SHAs. A head moves on a push or force-push. A base moves when the base branch advances or the pull request is retargeted. If the second read fails for any reason, including 401, 403, 404, or a rate limit, the result is `Partial(RevisionsUnverified)`. Content from mixed revisions is never marked complete.

The files and diff endpoints are mutable: they describe the pull request as it is now, not a fixed commit pair. The re-read narrows the window but has limits. A base or head that moves away and back to the same SHA between the two reads is not detected. Comparing two fixed SHAs (`GET /repos/{owner}/{repo}/compare/{base}...{head}`) would give an immutable diff. It is not in the allowlist yet, and its limits and its equivalence to the pull request diff have not been checked.

## Boundaries

- **Input.** `RepoRef` accepts `owner/repo` or a `https://github.com/owner/repo` URL. It rejects other hosts, ports, credentials in URLs, and extra path segments. `RepoPath` rejects absolute paths, `.` and `..` segments, empty segments, backslashes, and control characters. Each path segment is percent-encoded, so `?`, `#`, and `%` stay inside the path.
- **Pagination.** From GitHub's `Link: rel="next"` header, the client reads only the page number, and only when it is the next page in sequence. It rebuilds the URL from its own endpoint. GitHub's next links point at `/repositories/{id}/...`, and a URL taken from a response is never requested.
- **Redirects.** Redirects are never followed, so a credential cannot be replayed to another host. A 3xx response becomes `FetchError::Moved`.
- **Limits.** `Limits` caps items per page, page count, bytes per response, and total bytes per `fetch`. The total budget is checked before every read, and each read is capped at the smaller of the per-response cap and the remaining budget, so the last page cannot overrun it. Any limit set to zero is rejected with `InvalidLimits` before a request is made. When a cap is hit, or an error arrives after the first page, the result is `Completeness::Partial` with a reason, and the items already retrieved are kept.
- **File list ceiling.** GitHub lists at most 3000 files for a pull request. A file list shorter than the pull request's `changed_files` is reported as `Partial(FilesBelowChangedCount)`.
- **Mutable content.** Issues, comments, and pull request metadata can change between reads. A multi-page list is not an atomic snapshot. Each receipt records when its response arrived and the SHA-256 of its body.
- **Errors.** `FetchError` separates `InvalidCredentials` (401), `PermissionDenied` (403, with SSO-required and not-granted-to-token reasons), `RateLimited` (primary or secondary, on 403 or 429, following GitHub's documented headers), `Unavailable` (404: missing *or* not visible, never proof of nonexistence), `Moved`, `Gone`, `Transport`, `Malformed`, `ResponseTooLarge`, `ByteBudgetExhausted`, `InvalidLimits`, and `PaginationUnrecognized`. Error text never contains a token, a URL from a response, or a response body.
- **Secrets.** `SecretToken` has a redacted `Debug` and no `Display`. Only this crate can read the value.
- **Provenance.** Every `Fetched` names the repository it read. Every call yields a `Receipt` with method, host, path and query, status, GitHub request id, rate-limit headers, body size, body SHA-256, and the time the response arrived. A receipt never includes a credential or a body.
- **Auth is separate.** `auth::DeviceFlow` posts only to `github.com/login/device/code` and `github.com/login/oauth/access_token`. `EvidenceClient` never sends POST and never contacts github.com. `Credential` must be chosen explicitly. The crate never reads ambient credentials such as a `gh` login or environment tokens.

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
