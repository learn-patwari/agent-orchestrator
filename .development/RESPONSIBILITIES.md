# Who owns what (all agents)

One owner per thing. If a task does not clearly fit one row, the orchestrator picks the owner and says so in the brief. The same table is part of the Orchestrator's team snapshot (`backend/app/rules.py`).

| Agent | Owns | Never does |
|---|---|---|
| orchestrator | Splits work, briefs agents, routes, tracks status, usage and deploy-limit guards | Writes product code, merges, deploys |
| architect | SRS, phased plan, design notes, ADRs, behaviour-preserving refactors, risk flags | Implements features, writes user docs |
| developer_1, developer_2 | Product code, tests for their own change, the PR, API and schema docs of their own change; flag "affects guide" in the PR | Review or merge their own work, write user guides |
| tester | Test plans and cases, running them, reproducing bugs | Fixes product code, signs off a release before the docs gate is PASS |
| cicd | Release facts, release PRs, tag, deploys, the docs check, the deploy-limit guard | Product code, user guides |
| incident_reporter | Analyses incidents, files verified bugs | Fixes bugs, files unverified bugs |
| production_support | Fixes verified bugs with a regression test | New features, deploys |
| technical_writer | User guides, how-to and runbook prose, FAQs, PDFs, learning guides, release notes and changelog for every version | Product code, tests, deploy settings, bug triage, inventing facts |
| user | Approves the SRS, merges stage and main, decides on production | |

## Documents: owner of the facts, owner of the writing
| Document | Facts | Writing |
|---|---|---|
| SRS, design notes, ADRs | architect | architect |
| API and schema docs | developer of the change, same PR | developer |
| User guides, screenshots, FAQs, PDFs | the running app (local run) | technical_writer |
| Release notes, changelog | cicd (version, merged PRs, test counts, smoke) | technical_writer, every version |
| Test cases and reports | tester | tester |
| Runbooks | cicd, production_support | technical_writer when asked |
| Bug reports | incident_reporter | incident_reporter |

## Common rules
They are in every agent's system prompt and every brief (`RULES_FOR_ALL` in `backend/app/rules.py`): a bug found goes to the incident_reporter; a needed document goes to the technical_writer; design questions go to the architect; reuse the owning role's skills; stay in your lane; be honest and mark what is Not verified; no secrets.

## Guards enforced in code
- **Docs gate.** cicd runs `docs_check` (the platform runs the checks itself); the tester cannot sign off a release (`qc_result` with no failures) until it is PASS. Only the human can ship with a documented exception. Checks: release notes exist and the version matches `frontend/package.json` and `backend/VERSION`; the changelog entry is on top; every flagged PR has its guide updated or is an open gap with an owner; changed endpoints are in the API docs; links and image paths resolve; no secrets; Not verified items are repeated under "Needs your attention".
- **Merge gate.** The reviewer merges only into the integration branch; stage and main are the human's. Production needs the human's own approval from the app plus the architect's sign-off; nothing relayed by an agent counts.
- **Usage window guard.** At 98% of the 5-hour window new work stops, running work holds, and one resume is scheduled for after the reset. The app cannot read the window, so the person enters the percent and the reset time (Token profiles page).
- **Deploy-limit guard.** Every deployment is counted against the Hobby limit (100 a day); deploys stop at 70%.
