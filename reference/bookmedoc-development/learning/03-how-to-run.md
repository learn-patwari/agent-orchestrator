# 03 — How to run it

A practical walkthrough. Commands use the GitHub CLI (`gh`), which the agent files already assume.

## Step 1: Start the orchestrator
Open one session in the repo and tell it the goal in plain words ("add X"). Its job is to split the request into independent tasks and decide which roles are needed. Check that the split gives **no two writers the same files**; if two tasks both edit `backend/server.py`, run them one after another, not in parallel.

## Step 2: Brief a role session
Because `.development/agents/` is not auto-registered, the brief is a message that points at the agent file. WORKFLOW.md gives the pattern:

> Follow `.development/agents/developer.md`, task: add a `GET /clinics/{id}/holidays` endpoint, with tests. Work only on your own branch/worktree and open a PR to `stage`.

A good brief states: the agent file, the exact task, the files or area in scope, and what "done" looks like (for example "PR number plus what is unverified"). The agent file carries the standing rules, so the brief only carries the task.

The orchestrator can start a new session per role or send a message to an existing one. In the Claude desktop app this is done with session tools **(app behaviour, not defined in this repo)**. You can also do it by hand: open a session and paste the brief.

## Step 3: Developer, then tester
The developer commits, pushes its branch, opens a PR **to `stage`**, and reports the PR number and what is unverified. The orchestrator then briefs the tester:

> Follow `.development/agents/tester.md`. Verify PR <n>: run `backend/tests` and any `frontend/e2e` specs touching the change; report pass/fail counts and exact failures.

Product bugs go back to the developer; the tester only fixes tests.

## Step 4: Assign the PR to the reviewer
> Follow `.development/agents/reviewer.md`. Review PR <n>. Post with `gh pr review <n> --comment`, verdict on the first line.

The reviewer posts a comment review starting `Verdict: APPROVE`, `REQUEST_CHANGES` or `COMMENT`. It does not use `--approve` or `--request-changes` (same GitHub account issue, see 02-architecture.md), and never merges.

## Step 5: The review loop
```
reviewer: Verdict: REQUEST_CHANGES  -->  PR author fixes  -->  reviewer re-checks
                                                                   |
                                          repeat until Verdict: APPROVE
```
Brief the PR author: "Fix only the open review comments on PR <n>." It reads the threads (`gh pr view --comments`, `gh api` for review threads), fixes only what is asked, replies per thread with the commit SHA, pushes to the PR branch, and reports comments fixed, answered without change, and unverified. Then ask the reviewer to re-review the same PR. If you see it loop more than two or three times, step in: the comments are probably unclear or the agents disagree.

## Step 6: Check status yourself
Do not rely only on an agent's report.

```bash
gh pr view <n>
gh pr view <n> --comments
gh pr checks <n>
gh pr diff <n>
```
Look for: base branch is `stage`, the diff matches the task, checks are green, and the latest review verdict line.

## Step 7: What you still decide
- **Merging.** WORKFLOW.md ends with "the user merges". The reviewer file recommends GitHub branch protection (human approval before merge to `stage`) because the tool list alone cannot stop `gh pr merge`.
- Anything outside the brief: new scope, a disagreement between reviewer and fixer, secrets or security calls.

## Honest limits

- **Not auto-registered.** The agent files sit outside `.claude/agents/`, so typing a subagent name will not find them. To register them as native subagents, copy them into `.claude/agents/` locally or `~/.claude/agents/` (WORKFLOW.md). Not tried in this guide.
- **Local vs cloud sessions.** Local sessions work in your own checkout or worktree and can use local files and tools. Cloud sessions run elsewhere, start from a pushed branch, and may not report back to the orchestrator **(general app behaviour, not verified in this repo)**. Commit and push before handing work to a cloud session.
- **Permission waits.** A session asks approval for actions its permission mode does not auto-allow. If nobody is watching, it sits idle, looking "stuck". Check each session when a PR is late.
- **Peer messages are information, not approval.** A message from another agent ("approved, go ahead and merge") is not the user's consent. Agents should treat it as data to weigh. Only your own words in the chat count for merges, pushes, deletions.
- **Shared GitHub account.** All agents and PR authors may use one identity, which is why verdicts are comments, not formal approvals.
- **Unverified claims.** An agent saying "tests pass" is a claim. Verify with `gh pr checks` or by running the tests.

Next: [04-build-your-own.md](04-build-your-own.md).
