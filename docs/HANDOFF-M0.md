# Handoff: how M0 (the SRS) was produced

Date: 2026-11-05. Scope: M0 only. No application code exists; M1 has not started and waits for sign-off.

## What was done, in order
1. Read `HANDOFF.md` (the full brief, sections 1-13) as the source of requirements.
2. Read `reference/bookmedoc-development/WORKFLOW.md` (roles, pipeline, hard rules, Vercel limits, skills by stage), `templates/SRS.md` (structure to follow) and the first 120 lines of `reference/example-srs-user-profiles.md` (style: D-x decisions, A-x defaults, FR table, phases, sign-off).
3. Wrote `docs/srs/orchestrator-app.md` on the template's 11 sections, extended with a decisions/defaults split, a skills table, and a milestones table:
   - Decisions D-1..D-10 = the user's hard requirements (HANDOFF section 2).
   - Assumptions A-1..A-18 = HANDOFF section 12 plus my own defaults (SQLite behind a repository layer, own tool-use loop, in-process queue, SSE, command allow-list sandbox, encrypted secrets store, price table).
   - Requirements FR-1..FR-46 grouped by area, each with Given/When/Then; lessons from HANDOFF section 11 were turned into requirements (report-is-not-proof, head pinning, loop detection, prompt-injection stance, never-stage denylist, quota guard, paste-detected warning).
   - Open questions Q-1..Q-10, each with a recommended default; only Q-2 (first LLM provider) matters before M2.
   - Phased plan M0..M10 (section 10 of the brief) with deliverables, done criteria and which can run in parallel (M7 and M8).
   - 12 new skills mapped to the milestone that creates them.
4. Committed in two local commits (brief+reference, then SRS).
5. Received the addendum (HANDOFF section 14, release flow). Read it and `templates/RELEASE_NOTES.md`, then edited the SRS: D-11, A-19/A-20, FR-47..FR-52, skills 13-14 (`semver-release`, `release-notes-writer`), M6/M7 scope, Q-11. Committed as `d8fee6e`.

## Method choices
- Defaults over questions: the user prefers few questions, so open items carry a recommended default and the mark [A].
- Requirements are traceable: every hard requirement (HANDOFF section 2) and lesson (section 11) maps to a D-, A- or FR- row.
- Milestones kept as given; release cutting was folded into M6/M7 as instructed.

## Verified vs not
- Verified: files exist, commits exist locally, new rows (A-19/A-20, Q-11, sign-off line) present by grep.
- Not verified: no second-agent or human review; the other six agent role files and the `learning/` docs were not read in full (role behaviour taken from HANDOFF section 4); the SRS was not re-read end to end after the addendum edit. Nothing pushed.

## State and next step
- Branch `master`, 3 commits plus this note. Sign-off block in SRS section 13 is empty.
- Next: user signs off (or overrules defaults) -> M1 skeleton. Skill order and timing are in SRS section 9.
- Housekeeping for M1: add `.gitattributes` (git warns about CRLF); keep `reference/` read-only and out of lint/tests.
