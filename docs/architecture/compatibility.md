# Integration compatibility

Evidence date: 2026-09-29. This is a feasibility record, not a list of shipped features.

| Integration | Desktop | iOS and Android | Billing | Dependencies | Evidence status |
| --- | --- | --- | --- | --- | --- |
| OpenAI OSS Sign in with ChatGPT | Documented public-client flow and Responses inference | Direct loopback callback and lifecycle under investigation | Eligible ChatGPT plan usage | Local client, browser consent, platform secure storage; no mandatory Nzube account | Official protocol inspected; a private Linux experiment exercised protocol checks, not included as reproducible proof in this PR; consent and generation unverified |
| Codex app-server | Documented local host integration | Codex runtime has not been demonstrated on mobile | ChatGPT login or separately billed API mode; prototype permits ChatGPT only | Installed desktop Codex CLI and isolated Nzube-owned session; mobile would require a separately proven companion | Private persistence/cancellation/auth-failure experiment recorded; reproducible proof not yet included in this PR; real generation awaits separate login |
| GitHub Copilot SDK | Desktop runtime documented | Native runtime packaging unverified | Copilot subscription allowance; BYOK is a distinct API route | SDK runtime and GitHub/Copilot authorization | Documentation inspected; no Nzube runtime proof |
| Anthropic API / Agent SDK | API integration possible | Direct HTTP API transport possible; runtime proof absent | Separately billed API | Explicit provider credentials | Not implemented; do not promise third-party claude.ai subscription access without provider approval |
| GitHub App device authorization | Documented public-client device flow | Documented HTTP flow is portable; installed mobile flow unverified | Independent from the selected model provider | Dedicated GitHub App, installation and user consent | Adapter experiment underway; operator gh access does not prove product authentication |

No row represents a verified end-to-end Nzube integration yet. No automatic fallback from subscription to API billing is permitted.

## Provider choice under investigation

Prefer a direct, documented subscription flow over embedding an execution harness when it provides the required generation behavior. The OSS ChatGPT protocol uses PKCE, state, nonce, a stable opaque host identifier, dynamic client registration, an issued client ID, and ID-token verification. No confidential client secret ships in the app. The documented redirect is HTTP loopback at 127.0.0.1; accepting a custom mobile URL scheme has not been established.

Eligible Responses requests require streaming and store:false. History belongs to the client. A stream can fail after output starts; only a successful completion event creates a complete brief. Model availability and eligibility must be checked for the connected account.

A private Codex experiment, not included as reproducible proof in this PR, exposed a configuration risk: effective provider configuration can select a different route. It also found ambient agent instructions, skills, hooks and connectors that must be excluded. An isolated profile and explicit provider checks are necessary for that candidate. No user credentials were copied from another application.

## GitHub permissions

The candidate GitHub App requests read-only Contents, Issues, Pull requests, and metadata. Device-flow grants support refresh without embedding a client secret. User access remains constrained by the app permissions, installed repositories, the user's permissions, and organization policy. Permission errors, invalid credentials, throttling, network failure and partial retrieval must remain distinct.

## Framework decision remains open

| Candidate | Demonstrated by documentation | Remaining proof and maintenance cost |
| --- | --- | --- |
| Rust core, Tauri 2, React/TypeScript | Desktop and mobile shells, native Swift/Kotlin plugins | Real auth callback, platform secure storage and mobile streaming; build/install each target; maintain native glue |
| Shared TypeScript, Capacitor mobile, Electron desktop | Supported mobile shells and three desktop targets | Two shell integrations and desktop runtime cost; same auth/storage seams still need proof |
| React Native/Expo mobile, React/Electron desktop | Mobile platform storage and desktop shell coverage | Separate native and web UI integration; sharing TypeScript does not mean one UI runtime |

A desktop companion is a separate architecture choice, not implicit in a responsive interface. Use one only when a demonstrated provider requirement warrants it, and expose that dependency before connection. BAML is deferred until its runtime and provider support show a concrete benefit for repeated structured calls.

## Official sources

- [OpenAI OSS sign-in](https://developers.openai.com/siwc/token-sharing-open-source/sign-in)
- [OpenAI plan-usage limitations](https://developers.openai.com/siwc/token-sharing-open-source/preview-limitations)
- [OpenAI models and inference](https://developers.openai.com/siwc/token-sharing-open-source/models-and-inference)
- [Codex app-server](https://developers.openai.com/codex/app-server)
- [GitHub App user authentication](https://docs.github.com/en/apps/creating-github-apps/authenticating-with-a-github-app/authenticating-with-a-github-app-on-behalf-of-a-user)
- [GitHub App token refresh](https://docs.github.com/en/apps/creating-github-apps/authenticating-with-a-github-app/refreshing-user-access-tokens)
- [GitHub fine-grained permission table](https://docs.github.com/en/rest/authentication/permissions-required-for-fine-grained-personal-access-tokens)
- [Copilot SDK](https://github.com/github/copilot-sdk)
- [Tauri mobile plugins](https://v2.tauri.app/develop/plugins/develop-mobile/)
- [Capacitor](https://github.com/ionic-team/capacitor)
- [Electron](https://github.com/electron/electron)
- [Expo SecureStore](https://docs.expo.dev/versions/latest/sdk/securestore/)
- [React Native other platforms](https://reactnative.dev/docs/out-of-tree-platforms)
