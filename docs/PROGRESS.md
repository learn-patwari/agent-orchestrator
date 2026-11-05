# Progress checkpoint

Branches: `m1-skeleton` (PR 1), `m3-runtime` (stacked on it). Updated 2026-11-05.

## Done
- M0: SRS (`docs/srs/orchestrator-app.md`), sign-off recorded from the user's instruction to start development.
- M1 (partial): backend skeleton (FastAPI, SQLite document repository for all 14 entity tables, single-user password auth with signed cookie, settings with feature flags, encrypted secrets store with redaction and paste-scan endpoint), frontend skeleton (Next.js: sign-in/setup, Settings, Providers pages, `/api` proxy).
- M2: provider interface, OpenAI-compatible and Anthropic-compatible adapters (complete, streaming text, tool-call mapping, retries with backoff, error taxonomy), price table and cost accounting, per-role model mapping, test-connection endpoint and button, external-provider type hidden behind the disabled `external_providers` flag.

- M3 (backend): role loader (frontmatter + default tool allow-lists, validation), skill registry (ordered paths, progressive load, read-only), tool layer (jailed files, allow-listed shell without a shell, git read/write with never-stage denylist and no push/reset/clean/rebase), run loop persisted per step with resume, untrusted-data wrapping, loop detection, step/token limits, context compaction; API: GET /api/skills, /api/roles, POST/GET /api/runs.

## Verified
- Backend: 19 pytest tests pass (mock HTTP transport, no live LLM).
- Frontend: `tsc --noEmit` and `next build` pass.
- Manual smoke: backend + `next start` run together; setup, cookie auth, settings and the 401 guard work through the Next proxy.

## Not verified / known gaps
- No live call to a real LLM yet (needs a provider choice, SRS Q-2).
- No frontend tests or browser check of the pages; the UI was only built and typechecked.
- M1 gaps: no CI config, no project/feature endpoints yet, no per-role model UI, no Windows service script.
- Role-model endpoint exists but the UI for it is not built.

- M3 gaps: the shell allow-list includes python/node/npm, so it is a guardrail, not isolation (container isolation is later hardening); no UI for roles/skills/runs yet; protected-branch push refusal is by omission (no push tool), not yet tested against a real remote; compaction summary quality not tested with a real model; runs execute in a thread in the API process (no worker queue).

## Next
1. Live test of both adapters with a real key and base URL.
2. M4 orchestrator + inbox: chat, briefing, gates/approvals, assumptions, scheduler.
3. Add frontend tests (vitest, exclude `e2e/`) and a role-models UI.
