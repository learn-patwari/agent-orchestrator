# Progress checkpoint

Branches: `m1-skeleton` (PR 1), `m3-runtime` (PR 2, stacked), `m4-inbox-gates` (PR 3, stacked), `m4-orchestrator` (PR 4, stacked), `m5-ba-flow` (PR 5, stacked), `m6-pipeline` (PR 6, M6 complete except agent wiring, stacked on m5-ba-flow). Updated 2026-11-05.

## Done
- M0: SRS (`docs/srs/orchestrator-app.md`), sign-off recorded from the user's instruction to start development.
- M1 (partial): backend skeleton (FastAPI, SQLite document repository for all 14 entity tables, single-user password auth with signed cookie, settings with feature flags, encrypted secrets store with redaction and paste-scan endpoint), frontend skeleton (Next.js: sign-in/setup, Settings, Providers pages, `/api` proxy).
- M2: provider interface, OpenAI-compatible and Anthropic-compatible adapters (complete, streaming text, tool-call mapping, retries with backoff, error taxonomy), price table and cost accounting, per-role model mapping, test-connection endpoint and button, external-provider type hidden behind the disabled `external_providers` flag.

- M3 (backend): role loader (frontmatter + default tool allow-lists, validation), skill registry (ordered paths, progressive load, read-only), tool layer (jailed files, allow-listed shell without a shell, git read/write with never-stage denylist and no push/reset/clean/rebase), run loop persisted per step with resume, untrusted-data wrapping, loop detection, step/token limits, context compaction; API: GET /api/skills, /api/roles, POST/GET /api/runs.

- M4 complete (tests first, red confirmed per slice): inbox with recommended defaults, assumptions list with overrule, decide-with-defaults (never auto-approves gates), single-use audited gates; in-app notifications (question/approval items notify, duplicates suppressed); local scheduler (one-shot and recurring, runs once after a long gap, failing job isolated) paused at >=80% of an optional daily USD cap and resuming by itself, background loop in the app (`AO_NO_SCHEDULER` disables it); orchestrator chat (history, secret paste warning, redaction before store or send, rules + untrusted policy in system prompt), self-contained brief builder, per-role status table; workspace roots (default only `<data>/workspaces`) and a hard stop on new runs at 100% of the daily cap; UI pages Chat, Inbox (answers, approvals, assumptions, notifications), Agents.
- M5 (tests first, red confirmed per slice): BA/Architect flow. Intake calls the architect model for restatement, why, architect-view impact and at most 5 questions (invalid defaults repaired, bad JSON rejected without storing a feature); questions are posted one at a time (idempotent, never skips an open question); decide-with-defaults answers them as recorded assumptions but never approves the SRS; finalize produces a phased plan, assigns lanes (shared files share a lane and stack, dependency waves, cycle/unknown-dependency rejection), assembles the SRS from the template with D-n/A-n [A]/risks/sign-off block and versions it; the SRS approval is a human gate; developer runs are refused (409) unless the feature is approved. API: /api/features (+ /srs, /finalize), inbox actions advance features. UI: Features page. Skills added in `.development/skills/`: feature-intake-ba, srs-writer, phased-plan-lanes.
- M6 part 1 (tests first, red confirmed per slice): `backend/app/pipeline/`: git worktree manager (one worktree and `task/<name>` branch per task, stacked tasks branch from another task branch, name validation, refuses root inside the repo, refuses removing uncommitted work), merge gate (pure: base = integration branch, latest APPROVE on the CURRENT head via first-line `Verdict:` and second-line `Head:`, no unresolved medium+ findings, CI success, mergeable, tests with counts and none failing, no secret/production-config/reviewer-permission changes, review-round limit escalates; all failing reasons reported together), merge command with `--match-head-commit` and configurable style (never `--admin`), stacked-PR batch planner (base first, orphans and cycles skipped), `gh` client (PR facts, retarget, merge; runner injectable), batch executor (authority switch, never main/master/stage, retarget after base merge, head re-checked right before merge, dependents of a refused PR refused, audit events). Skill `merge-gate` added.
- M7 slice 1 (tests first): generic quota guard (`backend/app/quota.py`): per-platform limits table (Vercel Hobby seeded: 100/day, 100/hour, 1 concurrent build), rolling windows, stop at a threshold (default 70%) with a single notification, platform limit error stops everything for a day with no retries, unknown platform/metric is an error, custom tables for other platforms. Skill `quota-guard` added. Branch `m7-cicd` (stacked on m6-pipeline).

## Verified
- Backend: 172 pytest tests pass (mock HTTP transport and a scripted fake provider; no live LLM). The tests caught two real bugs, now fixed: Windows quote handling in the shell tool, and the never-stage denylist not unstaging in a repo with no commits (now uses `git rm --cached`).
- Frontend: `tsc --noEmit` and `next build` pass.
- Manual smoke: backend + `next start` run together; setup, cookie auth, settings and the 401 guard work through the Next proxy.

## Not verified / known gaps
- No live call to a real LLM yet (needs a provider choice, SRS Q-2).
- No frontend tests or browser check of the pages; the UI was only built and typechecked.
- M1 gaps: no CI config, no project/feature endpoints yet, no per-role model UI, no Windows service script.
- Role-model endpoint exists but the UI for it is not built.

- M3 gaps: the shell allow-list includes python/node/npm, so it is a guardrail, not isolation (container isolation is later hardening); no UI for roles/skills/runs yet; protected-branch push refusal is by omission (no push tool), not yet tested against a real remote; compaction summary quality not tested with a real model; runs execute in a thread in the API process (no worker queue).

- M4 gaps: the orchestrator does not yet call tools (it replies in text; delegating real runs from chat arrives with the pipeline in M5/M6); only `reminder` scheduled jobs exist; no tool calls the gates yet (merge flow is M6); "usage" for the pause is our own daily USD cap, not the Claude plan limit (the plan-limit pacing in docs/dev-plan-usage-windows.md is a separate, Claude-session-level mechanism); UI pages were built, typechecked and served (HTTP 200) but not opened in a browser or covered by frontend tests; no live LLM call.

- M5 gaps: the SRS is assembled from the template in code and the model only supplies analysis and plan JSON (the plan task file lists are the model's guesses, not checked against a repo); SRS is stored in the DB, not yet written into the project's `docs/srs/`; no SRS edit/re-generate/revision UI; UI not opened in a browser; no live LLM so prompt quality is untested.

- M6 slices 3-4 (tests first): dev/test/review state machine per task (developing -> dev_done -> tested -> approved -> merged; failed tests back to developing; REQUEST_CHANGES back to the PR author with round counting; limit escalates to the inbox; HOLD; a review of a stale head is refused), pipeline API (/api/pipeline/tasks, dev-done, tests, review, /merge) using settings `repo_path`, `worktree_root`, `github_repo`, `reviewer_can_merge` (default OFF; merge returns 403 when off or when the integration branch is main/master/stage), Pipeline page (tasks table, merge-approved-batch with refusal reasons).
- M6 remaining gaps: the pipeline records what happened but nothing yet DRIVES agents through it (a developer/tester/reviewer run is not yet started in the task worktree automatically, and test counts, PR numbers and reviews are posted through the API rather than read from runs); agents cannot push or open PRs (needs a gated tool); real `gh` never run (fake only); the Pipeline page was typechecked and built but not opened in a browser.

## Next
1. Live test of both adapters with a real key and base URL.
2. M7 remaining: Vercel adapter (status/logs, preview vs production), release cutting (SemVer suggestion, notes generation + edit UI, tag, GitHub Release, merge-to-main decision screen), smoke runner, built-in bug tracker, incident reporter and production support loop; skills semver-release, release-notes-writer, bug-lifecycle. Also: wire agents to the pipeline (developer run in the task worktree, tester run that records counts, reviewer run that posts the verdict) and a gated push/PR tool; then M7 (Vercel adapter, quota guard, releases, incidents, bug tracker).
3. Add frontend tests (vitest, exclude `e2e/`) and a role-models UI.

## M7 slice 2 (release helpers)
Done: `app/releases.py` (suggest_bump, next_version, generate_notes), skills semver-release and release-notes-writer. 179 backend tests pass (7 new, red first).
Not done in M7: Vercel adapter, release cutting endpoint/UI (tag, GitHub Release, merge-to-main decision), smoke runner, bug tracker, incident reporter, production support, bug-lifecycle skill.

## Decisions 2026-11-06 (user)
- Targets: web apps AND Windows apps (SRS D-12). M7 CI/CD adapter gets a Windows package/release kind next to Vercel; quota guard stays generic.
- Orchestrator ships as a Windows desktop app (SRS D-13): Electron shell proposed, Python backend as sidecar, new milestone M11. Not started.

## Decisions 2026-11-06 (user), part 2
- Code host: Bitbucket Server/Data Center, self-hosted (SRS D-14). Needs a Git-host interface; gh stays one adapter. Not started.
- Kubernetes deploy adapter (D-15). Open Q-12: deploy target only, or agents also run as k8s jobs (default: target only).
- Needs from user to build the Bitbucket adapter: base URL, project/repo key, auth type (HTTP access token assumed), whether Bitbucket approvals should count.

## Decision 2026-11-06 (user), part 3
- Agent builder and deployer, Foundry-style, as reusable plugins (SRS D-16). Plan change: pull M9 (plugin system + agent builder UI + export/install) ahead of the rest of M7; Bitbucket, Kubernetes, Windows-release and Vercel become plugins behind interfaces. Not started.

## M11 slice 1 (desktop shell)
Done: `desktop/` Electron shell (spawns backend + UI, window, per-user data dir, single instance, kills process tree on exit), `run-desktop.cmd`, `build-frontend.cmd`. Verified: `npm run smoke` passes (backend, UI and /api proxy). Not verified: the window itself was not looked at by me. Not done: installer/.exe, bundled Python, icon, auto-update, code signing.

## Provider without an API key (2026-11-06)
Done: `claude_cli` provider type (app/providers/claude_cli.py): runs `claude -p` with the user's own Claude login, CLI tools off, our tool calls via a JSON protocol (only offered tools accepted), ANTHROPIC_API_KEY removed from the child env, auto-finds the claude.exe bundled with the Claude desktop app. Providers page hides the key field for it. 190 backend tests pass (12 new, red first).
Verified live: the adapter finds and runs the real claude.exe and reports "not logged in" correctly. NOT verified: a successful real completion or tool call, because that claude.exe needs a one-time `/login` by the user.

## Provider presets (2026-11-06)
Done: `app/providers/presets.py` + `GET /api/providers/presets`: OpenAI, Anthropic, Claude login, Gemini, Microsoft Foundry/Azure, Mistral, Groq, xAI, DeepSeek, OpenRouter, Together, Fireworks, Perplexity, Cohere, Ollama, LM Studio, Custom (pick protocol + URL + key). Providers page uses a Provider picker. 196 tests pass. NOT verified: no vendor endpoint was called, so each preset URL is from vendor docs as I know them, unchecked live. Not covered: AWS Bedrock, Google Vertex (need signing/OAuth), custom extra headers.

## First user test findings (2026-11-06)
- Chat said "no model configured for the orchestrator role": the Providers page had no role-model assignment. Added "Models for roles" (use for all roles / per role). Failed connection tests now show the error text. Verified only by typecheck and build; not clicked in a window. The reported "auth error when adding" is unexplained (screen recording could not be viewed); most likely a Claude-login test before /login.

## Self-test in a browser (2026-11-06)
Ran a separate instance (own data dir, ports 8766/3766) and drove it in the browser pane: setup, Providers page (17 presets, 8 roles), add Claude-login provider, Test connection (now shows the real error), "Use for all roles", Chat. Found and fixed: default OpenAI preset left the URL empty. Result with no Claude login: Test connection and Chat both show "Claude login needed ... /login" (expected). NOT verified: any successful model reply or tool call (needs the user's one-time /login); Pipeline, Features, Inbox, Agents, Settings pages not clicked.

## UI v2 redesign (2026-11-06)
Reviewed the UI against the Web Interface Guidelines and rebuilt it: design system (tokens, light/dark, focus-visible, reduced motion, tabular numerals), app shell with sidebar (aria-current, skip link, responsive top bar on phones), shared Badge/PageHead/Empty/Alert, Home dashboard with setup checklist, Chat bubbles with Enter-to-send and thinking state, Providers in three steps with per-provider test and delete confirmation, table/empty states on Pipeline/Agents/Inbox/Features, settings cards. Verified: typecheck, build, and looked at Home, Providers and Chat (light, dark, phone width) in a browser. NOT verified: Features, Pipeline, Inbox, Agents, Settings screens visually; keyboard-only walkthrough; contrast audit with a tool.
