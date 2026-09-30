# Integration compatibility

Evidence date: 2026-09-29. This is a feasibility record, not a list of shipped features.

| Integration | Desktop | iOS and Android | Billing | Dependencies | Evidence status |
| --- | --- | --- | --- | --- | --- |
| OpenAI OSS Sign in with ChatGPT | Linux consent, secure storage, completed inference, persistence, and export exercised; macOS and Windows unverified | Actual provider login and completed inference unverified; loopback lifecycle remains under investigation | ChatGPT plan route inferred from granted OAuth scope and documented protocol; usage meter not inspected | Direct provider connection, browser consent, platform secure storage; no Nzube account, Codex runtime, or hosted backend needed for the demonstrated Linux route | Six real completed generations using `gpt-5.6-luna`; reproducible CLI and receipt proposed in [PR #2](https://github.com/AojdevStudio/nzube/pull/2) |
| Codex app-server | Documented local host integration | Codex runtime has not been demonstrated on mobile | ChatGPT login or separately billed API mode; prototype permits ChatGPT only | Installed desktop Codex CLI and isolated Nzube-owned session; mobile would require a separately proven companion | Private persistence/cancellation/auth-failure experiment recorded; reproducible proof not yet included in this PR; real generation awaits separate login |
| GitHub Copilot SDK | Desktop runtime documented | Native runtime packaging unverified | Copilot subscription allowance; BYOK is a distinct API route | SDK runtime and GitHub/Copilot authorization | Documentation inspected; no Nzube runtime proof |
| Anthropic API / Agent SDK | API integration possible | Direct HTTP API transport possible; runtime proof absent | Separately billed API | Explicit provider credentials | Not implemented; do not promise third-party claude.ai subscription access without provider approval |
| GitHub App device authorization | Device flow and connection lifecycle covered by fixtures; live product onboarding unverified | Installed mobile flow and adapter cross-compilation unverified | Independent from the selected model provider | Dedicated GitHub App, installation and user consent; platform credential-store implementation still needed | Read-only adapter proposed in [PR #3](https://github.com/AojdevStudio/nzube/pull/3); app registration awaits account security confirmation; operator gh access is not product authentication |

The Linux subscription proof is a working isolated integration. A complete Nzube product flow that combines dedicated GitHub authentication, repository evidence, and generation remains unverified. No automatic fallback from subscription to API billing is permitted.

## Completed Linux subscription boundary

The [public proof receipt](https://github.com/AojdevStudio/nzube/blob/1dc78c5eee64d8cf53533d57bcfa04e40de91d9d/tools/subscription-proof/RECEIPT.md) records consent, Secret Service credential storage, selected original guidance, account model discovery, and five Responses streams ending in `response.completed`. The complete output persisted and exported byte for byte. Cancellation after a delta, an interrupted stream, and an invalid bearer preserved the draft and the previous complete brief. These checks used a Nzube-owned authentication context and did not use Codex app-server or an API key.

The requests were synthetic. They contained no retrieved repository evidence. One guidance rule incorrectly asked a read-only investigation for a failing test. A revised source version restricted that rule to authorized implementation, and a real regenerated brief explicitly forbade creating or modifying tests. The refinement case returned the complete brief and changed exactly the requested line. These examples support the route and expose a prompt-quality defect; they do not establish broad output quality.

The [follow-up receipt](https://github.com/AojdevStudio/nzube/blob/f542322f91fb532db9159a86e12d0a613ad09540/tools/subscription-proof/RECEIPT.md#promoted-cli-run) records a further run through the public CLI at `d962cb2`. It exercised live token refresh, model retrieval, and a completed response. Its saved brief and export contained the same 3,236 bytes with SHA-256 `1521adb6d73f67af9a489c625927ebb45d501663af49b6d86af6eeef884f3425`. The read-only endpoint held, but the output classified some user-supplied facts as verified. Evidence classification still needs evaluation.

Remote sign-out, installed macOS and Windows operation, and actual mobile provider authentication remain unverified.

## Provider choice under investigation

Prefer a direct, documented subscription flow over embedding an execution harness when it provides the required generation behavior. The OSS ChatGPT protocol uses PKCE, state, nonce, a stable opaque host identifier, dynamic client registration, an issued client ID, and ID-token verification. No confidential client secret ships in the app. The documented redirect is HTTP loopback at 127.0.0.1; accepting a custom mobile URL scheme has not been established.

Eligible Responses requests require streaming and store:false. History belongs to the client. A stream can fail after output starts; only a successful completion event creates a complete brief. Model availability and eligibility must be checked for the connected account.

A private Codex experiment, not included as reproducible proof in this PR, exposed a configuration risk: effective provider configuration can select a different route. It also found ambient agent instructions, skills, hooks and connectors that must be excluded. An isolated profile and explicit provider checks are necessary for that candidate. No user credentials were copied from another application.

## GitHub permissions

The candidate GitHub App requests read-only Contents, Issues, Pull requests, and metadata. Device-flow grants support refresh without embedding a client secret. User access remains constrained by the app permissions, installed repositories, the user's permissions, and organization policy. Permission errors, invalid credentials, throttling, network failure and partial retrieval must remain distinct.

## Framework decision remains open

| Candidate | Demonstrated by documentation | Remaining proof and maintenance cost |
| --- | --- | --- |
| Rust core, Tauri 2, React/TypeScript | Desktop and mobile shells, native Swift/Kotlin plugins; private Android and iOS simulator proof apps were built and installed | Reference persistence and secure-storage sentinels have recorded simulator results; real provider consent, validated tokens, completed mobile inference, and production packaging still need proof; native Swift/Kotlin glue must be maintained |
| Shared TypeScript, Capacitor mobile, Electron desktop | Supported mobile shells and three desktop targets | Two shell integrations and desktop runtime cost; same auth/storage seams still need proof |
| React Native/Expo mobile, React/Electron desktop | Mobile platform storage and desktop shell coverage | Separate native and web UI integration; sharing TypeScript does not mean one UI runtime |

A desktop companion is a separate architecture choice, not implicit in a responsive interface. Use one only when a demonstrated provider requirement warrants it, and expose that dependency before connection. BAML is deferred until its runtime and provider support show a concrete benefit for repeated structured calls.

The current mobile lifecycle evidence is preliminary. An Android delayed fixture callback stalled after the app process froze during a seven-minute sign-in. A shorter callback completed. An iOS authentication-session fixture completed after 90 seconds, but the system-sheet cancel interaction remains unverified. A simulated callback does not prove that the provider accepts the browser session or that issued credentials can complete inference. No mandatory companion requirement or standalone mobile architecture has been established.

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
