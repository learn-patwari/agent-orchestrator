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

| Window | Milestone / content (lean scope) |
|---|---|
| W1 | M1 Skeleton + M2 LLM providers (one OpenAI-compatible adapter first; Anthropic-compatible second if budget allows) |
| W2-W3 | M3 Agent runtime (role loader, skill registry, sandboxed tools, persisted loop) |
| W4 | M4 Orchestrator chat, inbox, gates, assumptions (scheduler and notifications in-app only) |
| W5 | M5 BA/Architect flow (questions, SRS, plan with lanes) |
| W6-W7 | M6 Dev/Test/Review in worktrees, merge gate into `internal` |
| W8 | M7 core: quota guard, release cutting with notes preview, bug tracker (Vercel adapter read-only status) |
| W9 | M8 MCP client (Jira example deferred) + M9 plugin loader with the `software-delivery` plugin |
| W10 | M10 hardening: security review, full-suite clean-checkout run, docs |

Lean plan: about **10 windows (~2 days of continuous running, ~3-5 calendar days with weekly-limit pauses)**. Cuts that make this possible: parallel lanes (M7/M8/M9 run side by side in separate worktrees), combined small milestones per window, larger slices where tests are cheap, and deferring: Docker sandbox, email/webhook/phone notifications, Jira sync, extra CI/CD adapters, and polish on UI pages. Still unmeasured; re-estimate after W1. The 5-hour window is not the constraint; the **weekly cap** is, and parallel lanes spend it faster.

## 7. Gates still stay human
Resuming automatically never skips a gate: SRS sign-off, merge to `main`, production config, credentials and destructive actions wait for the user even when the schedule fires; the checkpoint marks such a task "blocked on user" and the resumed session moves to the next unblocked task or reports and stops.

## 8. Order of use of the user's limits
- Use windows for build work first; keep ~5% headroom for the user's own interactive questions (hence the 95% stop).
- Prefer cheaper models for read-only/search sub-tasks and keep the main model for design-heavy work.
- No LLM calls in loops without a cap (this protects both the plan limit and the app's own budgets, FR-29).
