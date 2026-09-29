# Core behavior contracts

Status: proposed contracts for the first implementation. No application framework has been selected and no product capability is certified by this document.

Nzube prepares an execution brief. Its output may instruct a receiving agent to open, merge, or deploy work, but the application exposes no repository mutation, execution, or dispatch operation.

## Intake and authorization

An editable request contains optional repository, request type, goal, context, endpoint, constraints, workflow preference, and harness preference. Rough notes are valid input. Unspecified values stay unspecified. A missing endpoint or contradictory authorization produces a focused question when it affects the requested work. It never implies permission to merge or deploy.

The request revision is monotonically increasing. Every edit is saved locally before generation. Selecting evidence or changing a workflow is an input edit. The user can inspect all content about to leave the device, including images, reference versions, repository excerpts, prior brief, and refinement instruction. Provider credentials are never part of that payload.

## Source library and request evidence

Reusable guidance belongs to the managed library. A request attachment or repository excerpt is request evidence. These are distinct roles even when their underlying bytes are identical.

An imported source has a stable identity, display name, media type, enabled status, and immutable content versions. Each version records a content hash, import timestamp, extraction status, and provenance. Updating a source creates a version; it never overwrites the version used by a historical brief. Selection resolves explicitly to enabled source versions at generation time. Disabling or removing a source excludes it from future snapshots. The interface must explain what historic data removal retains or deletes.

Text decoding and screenshot handling produce explicit success or failure. A filename is not extracted content. Images are sent only to a provider that supports them, with their inclusion shown to the user. Unsupported formats do not silently become empty text.

Relevant repository evidence includes files, agent instructions, issues, PR discussions, diffs, and history. Repository content records repository identity, immutable commit SHA when applicable, path or API resource, retrieval time, and content hash. Issues and discussion text additionally retain retrieval time because they can change without a commit. Failed, incomplete, rate-limited, and access-denied retrievals remain visible facts. A 404 cannot prove that a private repository is absent.

## Generation snapshot and context limits

Before inference, persist an immutable snapshot of the exact request revision, selected source versions, evidence, workflow settings, model, billing route, previous complete brief, and refinement instruction. The snapshot identifies omissions and why they occurred.

The provider adapter must reject an oversized payload before inference or request an explicit reduction. It must never silently truncate constraints. Provider-specific estimation must reserve space for output and disclose uncertainty. Summaries are derived evidence with lineage and do not replace the original authority of the request.

Application policy and user intent are separate from retrieved content. Guidance and evidence cannot grant permissions, change the user endpoint, select credentials, invoke tools, or alter the provider billing route. Generation has no execution tools.

## Attempts, completion, and recovery

Each attempt transitions from preparing to streaming and then exactly one of completed, cancelled, or failed. Persist partial text separately from complete revisions. An output becomes complete only after the provider reports successful completion; a disconnected stream is not success.

A failed or cancelled attempt preserves intake, source selections, partial output when available, and the last complete brief. On restart, unfinished attempts become interrupted and remain recoverable. Cancelling one attempt cannot complete or overwrite another. Only the request revision captured in the snapshot may be marked current by that attempt.

A brief is outdated when any effective generation input changes. Editing and restoring the same bytes may be recognized by the input fingerprint, but the interface must not label a response current merely because its generation ended after an edit. Library changes must invalidate affected snapshots even when the request was not opened.

Refinement creates a complete new brief from the prior complete brief, current intent and constraints, and the requested change. Unaffected requirements remain. Prior complete revisions can be inspected and recovered. Copy and Markdown export use the exact full text of the selected revision, including content outside the visible scroll area.

## Connection boundaries

Provider and GitHub connections have separate credentials, lifecycle, and disconnect actions. A provider connection names its billing route: subscription-backed or separately billed API. Failure never switches routes automatically.

GitHub repository operations are restricted to an explicit read endpoint set. Authentication uses its own protocol and may require POST. No general URL proxy, arbitrary GraphQL mutation, or repository write method is exposed. GitHub App permissions cover read-only Contents, Issues, Pull requests, and metadata. The app client ID is public configuration; no confidential OAuth client secret ships in the application.

Credentials use platform secure storage. Exports, model inputs, diagnostics, provenance, and fixture files never include them. Disconnect removes local access; remote revocation is a different action and must be described accurately. Authentication failure, authorization failure, rate limiting, and network failure are distinct outcomes.

## Portable guidance

Plain-language briefs work without PStack. Optional PStack guidance states outcome, context, constraints, proof, and authorized endpoint, leaving playbook choice to /poteto-mode. Babysit reaches merge-ready; Shipping lands authorized work; Autopilot-full manages independent PRs through merge; Autopilot-stack prepares coupled work without shipping; Orchestrate coordinates sustained programs. These meanings never enlarge the endpoint chosen by the user.

Loop-driven work preserves its finish predicate while resolving /loop syntax and availability for the selected receiving harness. Harness-specific commands are emitted only when their syntax and availability are established for the selected receiving harness. Files, tools, and skills used by Nzube are not presumed available to that agent. A handoff includes necessary context or tells the receiver exactly what remains to obtain.

## Proof required

The acceptance ledger is authoritative for completion. The interface must support keyboard navigation, screen readers, text scaling, visible focus, and narrow screens; desktop input/output comparison and separate mobile request/preview states must preserve accessible copy. Tests must cover source exclusion, immutable provenance, edits during streaming, preservation on failure and cancellation, complete refinement/export, read-only endpoint enforcement, and injection resistance. Prompt-quality evaluation compares the same cases and provider settings with and without selected guidance; schema validity and length do not measure quality.

The evaluation manifest in [evals/cases/core.json](../../evals/cases/core.json) and [rubric](../../evals/rubric.md) covers reproducible bugs versus unsupported diagnoses, behavior-preserving features, read-only investigations, handoffs, coupled stacks, contradictory context, denied repository access, injected evidence, constraint-preserving revisions, and non-PStack requests. Source exclusion and attachment failure have additional cases. All cases are unexecuted until actual provider outputs and reviewed scores are retained.
