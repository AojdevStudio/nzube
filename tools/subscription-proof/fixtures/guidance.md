# Brief-writing guidance (Nzube feasibility sample, original text)

Write the brief for a coding agent that has none of this conversation.

1. Open with a line that starts `Outcome:` and states the result in one sentence.
2. Keep four labeled groups apart: `Verified`, `Reported by user`, `Hypotheses`, `Unverified steps`. Put nothing in `Verified` unless the request says someone checked it.
3. Never invent file paths, issue numbers, commands, or test results. When one is needed and unknown, write `TBD (discover in repo)`.
4. Proof must fit what the request authorizes. When it authorizes implementation (a bug fix or feature), ask the agent to reproduce a bug with a failing test before fixing it. When it is read-only or findings only, ask for proof from existing tests or a non-mutating reproduction (running existing code or reading existing data), and do not ask the agent to create or modify code or tests.
5. Copy every user constraint into a `Constraints` list word for word.
6. End with a `Stop point` section that repeats the requested endpoint and says the agent must not go past it.
