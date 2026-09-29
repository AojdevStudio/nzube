# Requirements and acceptance evidence

All entries below are requested, not verified. Status at handoff: NOT IMPLEMENTED.

| ID | Requirement | Observable acceptance evidence |
| --- | --- | --- |
| NZ-01 | Structured and rough-note intake | Complete and partial requests persist, reopen, and retain all supplied constraints. |
| NZ-02 | Read-only GitHub context | Authorized private/public file and issue/PR retrieval succeeds; revoked access, partial results, and rate limits are disclosed. No write action is available in the product path. |
| NZ-03 | Managed guidance library | Import, inspect, update, disable, remove; excluded sources do not enter new generation. |
| NZ-04 | Source provenance | Output records identify source versions and repository revisions actually used. Missing context is disclosed. |
| NZ-05 | Grounded generation | Real provider output meets evaluation rubric; unsupported claims are not promoted to facts. |
| NZ-06 | Complete refinement | Requested edits appear, unaffected requirements survive, and full output copies/exports exactly. |
| NZ-07 | Drafts and history | Restart, cancellation, network loss, and provider failure preserve request and recoverable revisions. |
| NZ-08 | Subscription route | At least one supported existing-subscription route produces real output; billing and desktop/mobile limits are documented. |
| NZ-09 | Secure credentials and content boundaries | Revoke/disconnect works; logs and exports omit secrets; injected attachment instructions do not expand authority. |
| NZ-10 | Desktop delivery | Record build, install, and actual user-flow evidence separately for macOS, Windows, Linux. |
| NZ-11 | Mobile delivery | Real iOS/Android builds and emulator/simulator flows; device checks where available. Companion requirements are explicit. |
| NZ-12 | Accessible UX | Keyboard, screen-reader, text-scaling, focus, and narrow-screen core-flow checks. |
| NZ-13 | Workflow flexibility | Both PStack and non-PStack cases work; harness commands do not rely on fictional availability. |
| NZ-14 | Public release readiness | License decision, dependency inventory, redistribution review, reproducible packaging and credential-free docs. |
| NZ-15 | Store preparation | Submission materials, signing setup, and exact outstanding account/release steps; no claim of approval before approval. |
| NZ-16 | Human-led landing page | Accurate screenshots and evidence-backed capabilities delivered for the owner's design process. |

## Verification discipline

A green build does not prove the installed user flow. Exercise connect → import → generate → refine → copy/export → restart outside the checkout. Test expired auth, unsupported providers, cancellation, network failure, screenshot extraction failure, and source changes. Do not switch to separately billed APIs silently.

Report each platform as not started, built, installed, exercised, blocked, or unverified, with commands, artifact revision, and evidence. Future rows are not promises of completed integrations.

## Reviewable delivery

Use focused, dependency-ordered PRs. Finish the real vertical slice early. Complete independent work while external requirements are blocked. Final publication, merging, store submission, and deployment are distinct from preparation.
