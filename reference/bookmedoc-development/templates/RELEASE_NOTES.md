# BookMeDoc vX.Y.Z — <short friendly title>

Release date: YYYY-MM-DD · Cut from `internal` into `stage` · Previous version: vA.B.C
Recommendation: **MERGE TO MAIN / WAIT / DO NOT MERGE** — one sentence why, in plain words.

## In one minute
Two or three sentences a non-engineer can read: what is different for clinics, doctors, patients and admins after this release.

## What's new for people using it
- Plain-language bullets, one per user-visible feature. Say who benefits and where to find it ("Doctors can now ... under Profile").

## What we fixed
- Plain-language bullets per bug fix ("Signing out while viewing as another user now also ends the hidden admin session"). Reference the bug id if there is one.

## Behind the scenes
- Changes users do not see (tests, tooling, refactors, docs). Keep it short.

## Needs your attention before going live
- Database migrations or backfills (what runs, whether it is reversible).
- New or changed environment variables or Vercel settings.
- Anything that changes who can see or do what (roles, tenant isolation, audit).
- Breaking changes. Write "None" if none.

## How it was checked
- Tests that ran, with counts (backend pytest, frontend e2e, tsc, flake8) and the commit they ran on.
- **Not verified:** be explicit (for example "not run against the deployed stage URL", "Jenkins not run").
- Stage smoke result, if ci-cd ran it.

## Risk and how to undo it
- Risk level (low / medium / high) and why.
- Rollback: which release to go back to, and whether data changes block it.

## What is included
| PR | What it is | Type (feature / fix / docs / internal) |
|---|---|---|
| #123 | title | feature |

Version rule used: major = breaking or data contract step, minor = new user-visible feature, patch = fixes, docs, internal only.
