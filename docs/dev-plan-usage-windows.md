# Development plan for Claude: 5-hour usage windows with auto-continue

Status: Draft, applies after SRS sign-off (M1 onward). Date: 2026-11-05.

## 1. Goal
Build M1-M10 inside Claude's usage limits without losing work: when the 5-hour limit reaches **95%**, stop cleanly, save a checkpoint, schedule the continuation for just after the window resets, and resume by itself.

## 2. What was checked (2026-11-05)
- `get_usage` works in this app and returns, per window, `percentUsed` and `resetsAt` (UTC). Example reading: plan Pro, 5-hour 46% (resets 17:10Z), weekly 8% (resets 2026-11-12), extra usage off. It also returns this session's context fill (33% of 400k, auto-compact at 92%).
- **Not verified:** that a scheduled wake-up actually fires after a long wait and resumes work; that the schedule survives an app restart. Test this in the first window (see 6, step 0).

## 3. Rules (the control loop)
Check usage with `get_usage` **before starting each task** and **after each commit**.

| Condition | Action |
|---|---|
| 5-hour < 85% | Start the next task normally. |
| 85-94% | Do not start a new task; finish only the current slice if it is small, then commit. |
| **>= 95%** | Stop. Commit what is green, write the checkpoint (4), schedule the resume (5), notify the user, end the turn. |
| Weekly >= 90% | Stop, checkpoint, schedule resume for the weekly reset, notify the user (weekly resets take days; ask before changing plan). |
| Context >= 75% | Write the checkpoint and continue in a fresh session/compaction (do not wait for auto-compact at 92%). |
| Limit or 429 error mid-task | Do not retry in a loop. Checkpoint what exists (even a WIP commit on the task branch, not pushed), schedule resume. |

## 4. Checkpoint file
`docs/PROGRESS.md`, committed after every task (small commit), contains:
- current milestone and task, last green commit SHA, branch;
- what is done, what is next (ordered list of 1-3 tasks, each self-contained);
- how to run the tests and their last result with counts;
- open assumptions made since the last sign-off;
- usage reading at stop time and the planned resume time.
A resumed session reads `HANDOFF.md`, the SRS and `PROGRESS.md` first; it does not rely on chat history. Checkpoint format follows the `context-handoff` skill (created in M3; until then this list is the format).

## 5. Scheduling the resume
Resume time = `resetsAt` of the 5-hour window + 5 minutes buffer (read from `get_usage`, never guessed). Mechanisms, in order of preference:
1. **One-shot scheduled task** (scheduled-tasks tool) at that absolute time, with a prompt: "Read docs/PROGRESS.md and continue the plan in docs/dev-plan-usage-windows.md".
2. **Cron one-shot** (CronCreate) at the same time if 1 is unavailable.
3. **Chained wake-ups**: `ScheduleWakeup` is capped at 60 minutes per call, so re-arm it each hour until the reset time.
On wake: read `get_usage`; if the window has not reset yet, re-schedule for the new `resetsAt`; otherwise continue.

Constraints to state plainly (from the brief): schedules are local to the app, not cloud. They run only while the app is open and the computer is awake and not asleep; use keep-awake when waiting. If the app was closed, nothing resumes until the user opens it; the checkpoint makes that safe.

## 6. Work plan per window
Task sizing rule: **one task = one small, testable, committed slice (target <= 25% of a window)**, so a stop at 95% never leaves a half-done change. Commit locally per slice; push once per task when asked to.

- **Step 0 (first window, before M1):** after sign-off, run a 1-minute dry run of the schedule: create a one-shot task 2 minutes ahead that writes a line to `docs/PROGRESS.md`; confirm it fired. Calibrate: measure how many percent of a window one typical slice costs (percent delta around a small task) and record it in `PROGRESS.md`.
- Window estimates below are **rough and unmeasured** (Pro plan; limits are shared with the user's other Claude use, which can shrink them). Re-estimate after M1.

| Window(s) | Milestone / content | Cut points (safe stop points) |
|---|---|---|
| W1 | M1 Skeleton: repo layout, backend/frontend skeleton, DB models, auth, settings, clean-checkout install | after layout; after models+tests; after auth+settings |
| W2 | M2 LLM providers: interface, adapters, test-connection, cost table | after interface+fake server; after each adapter |
| W3-W5 | M3 Agent runtime: role loader, skill registry, tool layer, run loop + resume, compaction | after each of the five parts |
| W6-W8 | M4 Orchestrator + inbox: chat, gates, assumptions, scheduler with budget-aware resume, notifications | after gates; after scheduler |
| W9-W10 | M5 BA/Architect flow | after questions flow; after SRS generation |
| W11-W13 | M6 Dev/Test/Review: worktrees, runs, merge gate into `internal` | after worktrees; after merge gate |
| W14-W16 | M7 CI/CD, releases, incidents, prod support (can interleave with M8) | after Vercel adapter+quota; after release cutting; after bug loop |
| W17-W18 | M8 MCP | after client; after Jira example |
| W19-W20 | M9 Plugin system | after loader/validator; after UI+plugin |
| W21-W23 | M10 Hardening and docs | after security review; after full-suite run |

Roughly 20-25 windows. At a few windows per day that is about 1-2 weeks of calendar time if the weekly limit allows; with a Pro weekly cap it is likely to stretch to **3-5 weeks**. Weekly usage is the real constraint to watch, not only the 5-hour window.

## 7. Gates still stay human
Resuming automatically never skips a gate: SRS sign-off, merge to `main`, production config, credentials and destructive actions wait for the user even when the schedule fires; the checkpoint marks such a task "blocked on user" and the resumed session moves to the next unblocked task or reports and stops.

## 8. Order of use of the user's limits
- Use windows for build work first; keep ~5% headroom for the user's own interactive questions (hence the 95% stop).
- Prefer cheaper models for read-only/search sub-tasks and keep the main model for design-heavy work.
- No LLM calls in loops without a cap (this protects both the plan limit and the app's own budgets, FR-29).
