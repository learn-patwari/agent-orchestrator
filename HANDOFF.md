# HANDOFF: build the "Agent Orchestrator" web application

Written 2026-11-05 by the BookMeDoc orchestrator session for a brand-new, independent session. You have none of the earlier conversation. Everything you need is in this file and in `reference/`. This is a NEW product, not part of BookMeDoc: do not touch the BookMeDoc repository.

Owner: the user (akshaypattu24@gmail.com), a developer who ran this workflow by hand across many Claude Code sessions and now wants it as a standalone web app.

---

## 1. Goal in one paragraph
A web application that runs a team of AI agents doing software development, the way we did it by hand: a **main Orchestrator** is the only agent that talks to the human; behind it work a **BA/Architect**, **Developer(s)**, **Tester**, **Reviewer**, **CI/CD**, **Incident reporter** and **Production support**. The human gives a feature; the BA analyses it, explains, asks clarifying questions, writes an SRS and a phased plan; the human approves; then the pipeline runs (build, test, review, merge, deploy, incident handling) with the human informed through the orchestrator and asked only at defined gates.

## 2. Hard requirements from the user (do not drop any)
1. **Standalone web app.** Not tied to Claude Code or Claude Desktop sessions.
2. **LLM connection:** configurable by **API key + base URL** (OpenAI-compatible and Anthropic-compatible endpoints at minimum), model chosen per role. **Hide "external providers"** (anything that delegates to another product/CLI/hosted agent) behind a disabled feature flag for now; build the provider interface so they can be added later.
3. **MCP-ready:** any MCP server can be integrated later (Jira first in mind, also GitHub, Slack, etc.). Design an MCP client layer with per-role allow-lists now; ship without requiring any MCP.
4. **Same development environment/roles as below:** dev, tester, reviewer, cicd, prod support, architect (who is also the BA), and one main orchestrator who is the only voice to the human.
5. **Skills integration:** agents use skills from skill folders (paths in section 6). Build a skill registry/loader. **Create new skills where needed** (list in section 7).
6. **Plugin-style packaging:** a "work development" plugin = roles + skills + workflow + MCP config as one installable folder (spec in section 8). The BookMeDoc workflow becomes the first/default plugin.
7. **Feature intake flow:** when the user gives a feature, the BA produces a plan, explains it, asks questions (one at a time), and the architect view shows design implications (section 9).
8. Produce **a plan** (section 10) and execute it phase by phase, starting with the SRS for this app itself.
9. "Do not miss any tiny stuff": read section 11 (lessons and gotchas) before designing.

## 3. What exists to learn from (read these first)
All copied into `reference/`:
- `reference/bookmedoc-development/WORKFLOW.md`: roles, pipeline, hard rules, Vercel free-tier rules, skills by stage.
- `reference/bookmedoc-development/agents/*.md`: the 7 agent definitions (architect incl. BA/SRS mode, developer, tester, reviewer incl. merge conditions, ci-cd, incident-reporter, production-support). Note the old `pr-comment-fixer` role was removed on 2026-11-05 and replaced by production-support; review comments go back to the PR author.
- `reference/bookmedoc-development/templates/SRS.md`: SRS template.
- `reference/bookmedoc-development/learning/*.md`: concepts, architecture, how-to-run, build-your-own, exercises (the user wrote these to learn agent architecture; they explain the design intent).
- `reference/example-srs-user-profiles.md`: a real SRS the architect/BA produced (decisions D-x, defaults A-x, phases P0-P9, sign-off).

## 4. Roles (product behaviour to reproduce)
| Role | Does | Never |
|---|---|---|
| **Orchestrator** | Sole interface to the human. Splits work into independent lanes, briefs agents, tracks status, enforces gates, reports in one or two lines, keeps reminders and notes. | Writes feature code; merges; talks to the human via any other agent. |
| **Architect (BA mode)** | Clarifies requirements (interview-me: one question at a time, with a recommended default), writes SRS from template, phased plan, design notes, risk flags, behaviour-preserving refactor plans. Stops for human approval of the SRS. | Starts development before approval. |
| **Developer (N parallel)** | Implements one task on its branch with tests first, opens a PR to the integration branch. | Pushes to integration/main; touches another developer's files; checks deployments. |
| **Tester** | Writes/runs unit + e2e tests, reports failures with minimal repro, verifies incidents and fixes, can run flake8/lint. Raises bugs in the app's bug tracker when acting as a tester profile. | Fixes product code. |
| **Reviewer** | Multi-axis review (correctness, security/tenant isolation, tests, scope, UI). Posts a verdict on the first line. **May merge into the integration branch only when**: base is the integration branch; its latest review on the *current head commit* is APPROVE with no unresolved medium+ findings; CI green and mergeable; tests for the area passed with counts; no secrets/production-config changes. Merges in **batches**. Pins the head with `--match-head-commit`. | Merge to main; merge docs that change production config; push code. |
| **CI/CD** | Deploys the integration branch, tracks status and logs, runs integration tests against the deployed URL (read-only), counts platform quota first. Owns deployment config. | Change production settings or env without human approval. |
| **Incident reporter** | Analyses a failure/incident, asks the Tester to verify, then files verified bugs in the bug tracker with severity and evidence. | File unverified bugs; touch product code. |
| **Production support** | Takes verified bugs from the tracker (from incident reporter or any tester profile): reproduce, failing test, smallest fix, PR referencing the bug id, then asks tester to re-verify. Escalates critical/high immediately via the orchestrator. | Refactor; change production config. |

PR authors fix review comments on their own PR (no separate comment-fixer).

Pipeline: Orchestrator splits work, **Architect SRS (human approval)**, Developer builds, Tester verifies, Reviewer reviews (changes go back to the author, then re-review), Reviewer batch-merges, CI/CD deploys and smokes, failures to Incident reporter, Tester verifies, bug filed, Production support fixes, Tester re-verifies and closes, Orchestrator reports.

## 5. Product shape (proposal, mark [A] = assumption the user can change)
- [A] Stack matching the user's habits: **Next.js (React) frontend + FastAPI (Python) backend + MongoDB** (SQLite acceptable for MVP). Single-user/local-first first; multi-user later.
- **Agent runtime**: per-role loop (system prompt from the role file + selected skills + tool access) calling the configured LLM, tool-use loop with tool allow-lists, context compaction with handoff summaries, resumable runs persisted in the DB.
- **Workspace manager**: git worktrees per task/branch; command sandbox/allow-list; never run in the user's main checkout.
- **Event bus + run log** (every prompt, tool call, cost, decision), searchable; **audit trail** for approvals and merges.
- **Human inbox**: one place where the orchestrator posts status, questions with recommended defaults, approval requests; supports "decide with defaults" mode that records every assumption in a list the user reviews later.
- **Notifications**: pluggable (browser, email/webhook; phone push later).
- **Scheduler**: one-shot and recurring check-ins (e.g. "continue at 1 AM"), usage-aware: if LLM budget >= 80%, wait and resume.
- **Budgets and limits**: token/cost budget per run/role/day, concurrency lanes, loop detection (more than 2-3 review rounds means stop and escalate), platform quota guards (a generic "quota guard" so e.g. the Vercel 100-deployments/day limit is respected).
- **Integrations**: Git (local), GitHub (token or `gh`), bug tracker abstraction (built-in tracker + Jira via MCP later), CI/CD provider adapter (Vercel first because BookMeDoc uses it; adapter interface so others fit).
- UI pages: Chat with orchestrator, Feature board (intake, SRS, plan, tasks by lane), Agents (live status, last activity), PRs/merge queue, Runs & logs, Skills & Plugins, Providers (LLM) & MCP servers, Budgets & settings, Assumptions list.

## 6. Skills: where they live and how to integrate
Skills are folders containing a `SKILL.md` with YAML frontmatter (`name`, `description`) plus optional reference files. They are loaded progressively: the registry exposes name+description to the model; the body is loaded only when a skill is invoked.

- **Personal skills directory (this machine):** `C:\Users\admin\.claude\skills\<skill-name>\SKILL.md`
- **Plugins directory:** `C:\Users\admin\.claude\plugins\` (also `...\plugins\marketplaces\` and `...\plugins\store\`); `installed_plugins.json` lists installed ones. The `engineering:*`, `small-business:*` and `anthropic-skills:*` skills come from plugins.
- **Project skills (to create for the new app):** `<project>\.development\skills\<skill-name>\SKILL.md` and plugin-local `plugins\<plugin>\skills\`.
- Registry must support multiple configured search paths with precedence: plugin-local, project, personal. Never write into someone else's skills folder; copy/import instead.

Skills installed in `C:\Users\admin\.claude\skills\` (use by stage):
- Orchestrator: `using-agent-skills`, `context-engineering`, `planning-and-task-breakdown`, `interview-me`
- Design/BA/Architect: `idea-refine`, `spec-driven-development`, `documentation-and-adrs`, `doubt-driven-development`, `deprecation-and-migration`
- Build: `incremental-implementation`, `test-driven-development`, `api-and-interface-design`, `frontend-ui-engineering`, `source-driven-development`, `code-simplification`
- Verify: `browser-testing-with-devtools`, `debugging-and-error-recovery`, `constraint-driven-development` (and plugin skill `engineering:testing-strategy`)
- Review: `code-review-and-quality`, `github-pr-review`, `security-and-hardening`, `web-design-guidelines`, `performance-optimization`
- Ship: `git-workflow-and-versioning`, `ci-cd-and-automation`, `shipping-and-launch`, `observability-and-instrumentation`, `deploy-to-vercel`, `vercel-cli-with-tokens`
- Also present: `writing-guidelines`, `vercel-optimize`, `vercel-react-best-practices`, `vercel-composition-patterns`, `vercel-react-native-skills`, `vercel-react-view-transitions`, `reverse-engineer-anything`, `cab-invoice-zip-summary` (unrelated, ignore).
Each agent role file lists its skills (see `reference/bookmedoc-development/agents/*.md`, "Skills" section). The app must let a plugin map role to skills and let the user edit it.

## 7. New skills to create (put in `.development/skills/` of the new project; follow the skill-creator conventions)
1. `feature-intake-ba`: turns a raw feature into questions, assumptions with recommended defaults, scope, SRS; one question at a time; "decide with defaults" mode.
2. `srs-writer`: fills `templates/SRS.md`, decisions D-x, assumptions A-x, phases, sign-off block.
3. `phased-plan-lanes`: splits a plan into phases/tasks, marks which can run in parallel and which touch the same files (lanes, stacked PRs).
4. `agent-role-authoring`: how to write a role definition (frontmatter, scope, hard rules, skills, outputs).
5. `plugin-authoring-orchestrator`: how to author/validate a work-development plugin (manifest, roles, skills, workflows, MCP).
6. `llm-provider-adapter`: how to add an LLM provider (API key + base URL), streaming, tool-calling mapping, retries, cost accounting.
7. `mcp-integration`: add an MCP server, scope it per role, secrets handling, tool allow-lists.
8. `orchestrator-runbook`: briefing agents, collecting reports, verifying reports against the repo (a report is not proof), nudging idle agents, gates and escalation.
9. `merge-gate`: the reviewer's merge conditions as a checklist, batching, stacked PR order, head pinning.
10. `quota-guard`: reading platform limits (e.g. Vercel Hobby) and budgeting pushes/merges/deploys.
11. `context-handoff`: writing a handoff like this file for a fresh session or agent.
12. `bug-lifecycle`: bug statuses, severities, who moves them, incident to verified to fixed to re-verified.
Create only what the plan needs when it is needed; each skill must include a "when to use" description.

## 8. Plugin spec to implement (the "work development" plugin)
A plugin is a folder, version-controlled, installable/removable in the UI:
```
plugins/<name>/
  plugin.json        # name, version, description, entry workflow, required capabilities
  roles/*.md         # role definitions (frontmatter: name, description, tools, skills, model hint)
  skills/<skill>/SKILL.md
  workflows/*.yaml   # pipeline stages, gates, transitions, batching rules
  templates/*.md     # SRS etc.
  mcp.json           # optional MCP servers, per-role allow-lists (no secrets inside)
  policies.md        # hard rules (branch, merge, secrets, quota)
```
Ship `plugins/software-delivery/` = the BookMeDoc workflow generalised (remove BookMeDoc-specific product roles, Vercel specifics go to a CI/CD adapter). Validate plugins on load (schema, missing skills, role tool lists).

## 9. Feature intake flow (core UX)
1. Human types a feature to the Orchestrator.
2. Orchestrator hands it to the **Architect in BA mode**.
3. BA replies with: restatement, why it matters, impact on existing system (architect view: affected modules, data, security, risks), then **questions one at a time**, each with options and a recommended default; user can answer, accept the default, or switch on "decide with defaults" (then every assumption is recorded as `[A]` in the SRS and a review list).
4. BA writes the SRS (template), explains it in plain language, proposes phases P0..Pn with lanes (what can run in parallel, what blocks).
5. Human approves the SRS (gate). Only then does the Orchestrator start Developers.
6. Orchestrator reports progress; the human merges nothing unless a gate says so (merge to main, production config, credentials, destructive actions stay human).

## 10. Plan for building this app (do in this order; first deliverable is an SRS for this app, dogfooding the flow)
- **M0 Discovery/SRS**: read `reference/`, write `docs/srs/orchestrator-app.md` using the template; list open questions with recommended defaults; stop and ask the user to sign off. Ask only genuinely blocking questions.
- **M1 Skeleton**: repo layout, backend + frontend skeleton, DB models (projects, features, tasks, agents, runs, messages, approvals, assumptions, budgets, providers, mcp_servers, plugins), auth for single user, settings page.
- **M2 LLM providers**: provider interface, OpenAI-compatible and Anthropic-compatible adapters (API key + base URL, per-role model), cost accounting, test-connection button; external providers behind a hidden flag.
- **M3 Agent runtime**: role loader, skill registry (search paths, progressive load), tool layer (shell in sandbox, files, git, gh), run loop with allow-lists, persistence and resume, context compaction.
- **M4 Orchestrator + human inbox**: chat UI, briefing agents, agent status, gates/approvals, assumptions list, notifications, scheduler with budget-aware resume.
- **M5 BA/Architect flow**: feature intake UI, questions, SRS generation and approval, phased plan with lanes.
- **M6 Dev/Test/Review pipeline**: worktrees, developer runs, tester runs, reviewer verdicts, merge gate with batching and head pinning, stacked PR handling.
- **M7 CI/CD + incidents + production support**: deploy adapter (Vercel first), quota guard, integration test runner, incident reporter, built-in bug tracker, production support loop, tester re-verify.
- **M8 MCP**: MCP client (stdio/http), per-role allow-lists, UI to add servers; Jira as the first example integration.
- **M9 Plugin system**: plugin loader/validator, install/remove UI, ship `software-delivery` plugin.
- **M10 Hardening**: security review (prompt injection, secrets, sandbox), budgets, observability, docs, tests across all milestones.
Each milestone: tests first, PR-style small commits, a short report of what is verified vs not.

## 11. Lessons and tiny details learned the hard way (build these in)
Process:
- Agent definitions are **not auto-registered**; the orchestrator delegates by giving the agent its role file plus a task brief. In the app, roles are first-class.
- **Briefs must be self-contained** (task, files/area, done criteria); fresh agents have no history.
- **A report is not proof.** Verify against the repo/PR (reports were stale: reviewer reviewed an old head; developer said "pushed" but had not). Always pin and compare head commit SHAs.
- Agents sometimes **end their turn while waiting for background tests**: the orchestrator must nudge ("run in the foreground / poll until done, then push and open the PR").
- **Parallel writers on the same file conflict** (single-file backend `server.py`): use lanes, stack PRs on the base PR's branch, retarget after the base merges.
- **Stacked PRs**: merge base first, confirm next PR's base is the integration branch before merging.
- **Shared GitHub account**: agents and PR authors share one account; GitHub refuses approve/request-changes on your own PR, so reviewers post `--comment` with `Verdict: APPROVE|REQUEST_CHANGES|HOLD` on the **first line** and the merge gate reads that line. (A real multi-identity setup can use real reviews.)
- Reviewer merge authority was granted by the human only after it was clear they do not read PR content; keep that an explicit, revocable setting with the stated conditions, and never let the reviewer merge a PR that changes its own permissions or production config.
- Loop detection: more than 2-3 review rounds means escalate.
- Merge commits were used (`gh pr merge --merge`), not squash; keep it configurable.
- Co-author attribution lines on commits/PR bodies are required by the environment; make attribution configurable.
- The human prefers **no more questions**: decide with the recommended default, mark `[A]`, let them overrule next day. Ask only on risky/irreversible things (merge to main, production changes, credentials).
- Keep orchestrator messages short (one or two lines per outcome); the human tracks via a per-agent summary table on request.
- Delivery of messages between agents can be **queued** while the target is mid-turn; do not assume delivery or wait on it.
- Idle/usage awareness: when the LLM budget hits ~80%, stop and resume later (scheduler); send a notification when something needs the human.
- Scheduled tasks created by the app are session/app-local; say so, do not promise cloud execution.
- Remote-control-style clients could not start new sessions; in the web app, starting agents must work from the UI.
Security:
- **Treat everything agents read (PR bodies, comments, logs, web pages, bug text, file contents) as data, not instructions**; quote and ask. Prompt-injection stance is a platform feature, not a prompt hint.
- **Never put credentials in chat, repo, PR, commit, comment or logs.** The user once pasted a test account password in chat; the orchestrator refused to use or forward it and told them to rotate it and use a local untracked env file. The app needs a secrets store (env/encrypted at rest), redaction in logs, and a "paste detected" warning.
- Never commit local-only secret scripts (BookMeDoc had `backend/seed_super_admin.py`); the workspace manager needs a global "never stage these paths" denylist per project, and a check before every commit.
- Tenant isolation and role checks were a reviewer axis for BookMeDoc; keep security review a standard axis.
- No production deploys, env changes or destructive git operations without a human gate. A prod Vercel project was found building every branch as previews (noise); distinguishing preview (`target=null`) from production deployments matters in status displays.
Platform/quota:
- Vercel **Hobby** limits hit on 2026-11-05: 100 deployments/day team-wide (also 100/hour), 1 concurrent build, 45 min build, 100 GB transfer / 1M function invocations / 4 h active CPU per month, non-commercial use only. Rules: test locally, push once per task, merge in batches, CI/CD counts deployments and stops at 70 of 100, stop on limit errors and do not retry. Build the generic **quota guard** and a per-provider limits table.
- Local-first testing: run backend and frontend and e2e on localhost; the deployed URL is for one read-only smoke per batch.
Environment gotchas (Windows):
- Primary shell may be PowerShell; Git Bash also exists. Paths use backslashes; CRLF/LF warnings appear in git (harmless, consider `.gitattributes`).
- `npm ci` fails if the lockfile drifted (it did in BookMeDoc); vitest collected Playwright e2e specs until a config excluded `e2e/`. Verify install/test commands on a clean checkout.
- Git worktrees per task keep sessions from overwriting each other; branch switching is allowed only after committing/parking work and never in a checkout another session uses.
- Bug tracker (BookMeDoc): statuses open/triaged/in_progress/resolved/closed/reopened/wont_fix, severities low/medium/high/critical; tester role may create/read/comment/patch; credentials via a local env file. Build the same lifecycle into the app's built-in tracker.

## 12. Assumptions to confirm or change later ([A])
- [A] Stack: Next.js + FastAPI + MongoDB (SQLite for MVP).
- [A] Single human user first; no multi-tenant.
- [A] Providers: generic OpenAI-compatible + Anthropic-compatible via base URL + key; external providers hidden behind a disabled flag.
- [A] Git host: GitHub first; local git always.
- [A] CI/CD adapter: Vercel first; others later.
- [A] Reviewer merges into the integration branch; main stays human.
- [A] Merge style: merge commits.
- [A] Built-in bug tracker; Jira later via MCP.
- [A] Notifications: in-app first; email/webhook then phone push later.

## 13. How to start (your first actions)
1. Read this file, then `reference/bookmedoc-development/WORKFLOW.md` and the 7 agent files, the SRS template and the example SRS.
2. Create `docs/srs/orchestrator-app.md` (M0) with decisions, `[A]` assumptions (section 12), open questions with recommended defaults, phased plan (section 10) and a sign-off block. Do not start M1 until the user signs off the SRS.
3. Keep reports short; state what is verified and what is not.

---

## 14. Addendum (same day): version-based releases and an internal branch
Requirement from the user, to be part of the app and of the default `software-delivery` plugin: every change that reaches `stage` is **versioned with humanized release notes** so the human can decide whether to merge to `main`, and an **internal branch** collects smaller features and bug fixes.
- Branches: task branch -> PR -> `internal` (reviewer batch-merges) -> release PR -> `stage` (versioned, deployed, smoke tested) -> `main` (human only). Branch names are plugin settings.
- SemVer: major = breaking or data-contract step, minor = new user-visible feature, patch = fixes/docs/internal.
- Release mode (CI/CD role): classify merged PRs, pick the bump, write `docs/releases/vX.Y.Z.md` + `CHANGELOG.md` entry from a release-notes template, open `internal` -> `stage` PR, tag the merge commit, create the GitHub Release, deploy, run one read-only smoke, then open `stage` -> `main` with a MERGE / WAIT / DO NOT MERGE recommendation. The human decides.
- Release notes are for a decision-maker: one-minute summary, what's new for users, what was fixed, behind the scenes, **needs your attention** (migrations, env/config, roles/tenant changes, breaking), how it was checked with counts and an explicit **not verified** list, risk and rollback, included PRs. Never invent test results.
- Never rewrite or delete a published tag; fix forward with a new patch version. Hotfix releases for critical/high bugs are allowed on their own.
- A working example is in `reference/bookmedoc-development/` only after BookMeDoc PR #30 merges; until then see `WORKFLOW.md` section "Release flow" and `templates/RELEASE_NOTES.md` on branch `ap-dev/repo-session-orchestrator-6386a6` of the BookMeDoc repo (copy the current text into your SRS; do not touch that repo). Add the milestone: M6/M7 must include release cutting, notes generation (UI to preview and edit the notes), version bump suggestion, and a "decide: merge to main?" screen.
- New skills to add to section 7: `release-notes-writer` (humanized notes from PRs) and `semver-release` (bump rules, tags, changelog).
