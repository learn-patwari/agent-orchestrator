# Team workflow (proposal, not built yet)

Redesign of the team and the delivery pipeline, as drawn in `office-view-mockup.html`.
Status: proposal. Nothing in the backend or role files has changed yet.

## Roles

| Role | Does | Replaces / changes |
|---|---|---|
| Orchestrator (manager) | Assigns tasks to developer 1 and 2, tracks the schedule, asks the human before production | unchanged |
| Architect | Reviews all code after the developers sign off; signs off production | was Architect/BA |
| Developer 1, Developer 2 | Build, unit test, peer review each other's pull requests | Developer + Reviewer merge into two developers |
| CI/CD | Deploys to QC after architect review; deploys to production after architect sign-off | unchanged role, two environments |
| Tester (QC) | UAT, system and integration tests, only after the developer's unit tests; signs off | was Tester |
| Incident reporter | Watches the test reports in the app, raises incidents to production support | gets a monitoring job |
| Production support (L1 to L4) | Triage and minor fixes; the fix is reviewed by the developer who owns the feature | levels L1 to L4 inside one role at first |

Not agents: the **office boy** is the app's message passing (briefs, pull requests, builds, reports move between agents with
no model call). The **receptionist** and the **cat** are cosmetic.

## Release cycle

1. Orchestrator checks the schedule and sends briefs to developer 1 and developer 2.
2. Each developer builds the feature and runs unit tests.
3. Peer review: developer 2 reviews developer 1's pull request, developer 1 reviews developer 2's.
4. Architect reviews the approved code.
5. CI/CD deploys to QC.
6. Tester runs UAT, system and integration tests and signs off.
7. Orchestrator checks the schedule and **asks the human** for the production deployment (gate).
8. Architect signs off production.
9. CI/CD deploys to production.

## Production issue cycle

1. Incident reporter sees a problem in the test reports and raises an incident to production support.
2. Production support makes a minor fix (L1 to L4 as needed).
3. The developer who owns the feature reviews the fix.
4. Architect reviews it, CI/CD deploys it to QC, the tester retests, and the cycle repeats (step 7 onward for production).

## Gates that stay

Merge to `main`, production deployment and credentials always need the human. Developers cannot start without an approved
SRS. Budget and workspace limits apply to every run. Deployment itself stays an in-app plugin built only when asked (D-16).

## What would change in the app

- Role files: rename `reviewer` to `developer-2`, `developer` to `developer-1`; add `production-support`; keep `tester`, `ci-cd`, `incident-reporter`, `architect`.
- Orchestrator: assigns to a named developer; knows the pipeline order and the human gate before production.
- Pipeline state: per feature, record stage (build, peer review, architect review, QC, production approval, production, monitoring).
- Environments: two named targets (QC and production), each with a current version.
- Office boy: a run-feed (`/api/runs` plus hand-offs) that the Agents tab animates; no new model calls.

## Open questions

- Should L1 to L4 be four agents or one agent that escalates through levels?
- Who watches the test reports: a scheduled run of the incident reporter, or a trigger when a report lands?
- Can both developers run at the same time (parallel runs on one workspace), or each in its own workspace copy?
