# 01 — Concepts

Plain-language definitions, each tied to something in this repo.

## Agent
An AI model running in a loop: it reads instructions, decides on an action (read a file, run a command, edit code), sees the result, and repeats until the task is done. What makes it "a developer agent" or "a reviewer agent" is the instructions and tools you give it. In BookMeDoc an agent is a markdown file such as `.development/agents/reviewer.md`.

## Subagent
An agent started by another agent to do a side task and return a result. Claude Code can register agent files as native subagents if they sit in `.claude/agents/` (or `~/.claude/agents/`). The BookMeDoc files deliberately live in `.development/agents/`, so they are **not** auto-registered (WORKFLOW.md says so). They are used as briefs instead.

## Session
One running conversation with Claude, with its own context (memory of what it has read and done). A session is long-lived: you can talk to it, stop it, or send it follow-up messages. In this repo's setup, each role runs as its own session; the orchestrator is another session.

Why that matters: a session has a limited context window. A fresh session for the reviewer means the reviewer arrives with no author bias and no clutter.

## Worktree
A git feature: a second checkout of the same repository in another folder, on its own branch. Two sessions can edit "the same repo" without overwriting each other because each has its own folder. Branches follow the task: an agent may switch branches, but must commit or park work first and never switch in a checkout another session is using, because that would silently change the files it is editing.

## Orchestrator
The coordinating session. It reads the user's request, splits it into independent tasks, briefs role sessions, tracks PR status, and reports to the user. Per WORKFLOW.md it makes sure "no two writers touch the same files". Its own skills are planning-oriented: `using-agent-skills`, `context-engineering`, `planning-and-task-breakdown`, `interview-me`.

## Skill
A packaged set of instructions (for example `test-driven-development`) that an agent loads on demand with the Skill tool. Think of it as a reusable playbook. Agent files end with a "Skills" section naming which playbooks that role should invoke. A skill is not code the agent runs; it is guidance that enters the agent's context when invoked.

## MCP tool
MCP (Model Context Protocol) is a standard way to plug external capabilities into an agent: GitHub, Slack, a browser, a database. Each capability appears to the agent as a named tool. The repo's agent files don't configure MCP servers themselves; the `tester` mentions browser testing via the `browser-testing-with-devtools` skill, which depends on a Chrome DevTools MCP server being configured **(from the skill description; not verified in this repo)**.

## Permission mode
A setting on a session controlling what it may do without asking you (e.g. ask before each edit or command, or auto-approve some). It is separate from the agent file's `tools:` line. Two layers exist:
- **Tool allowlist** in the agent file (the reviewer has `tools: Read, Grep, Glob, Bash`, so no Edit/Write).
- **Permission mode** of the session, set by the app or user. If a session needs approval and nobody answers, it waits (see 03-how-to-run.md).

## One agent or many?

Start with one. Add agents only when you hit a real problem:

| Problem with one agent | What many agents give you |
|---|---|
| The author reviews their own work and misses flaws | Separate reviewer with a fresh context and no write tools |
| Context fills up on a big task | Each role keeps a small, focused context |
| Independent tasks wait in a queue | Parallel sessions on separate worktrees |
| One agent has all powers, so a mistake is costly | Narrow tools and scope per role |

Costs of many agents: more coordination, more tokens, more places for instructions to drift, and merge conflicts if two touch the same files. Rule of thumb: if the task fits in one sitting of one person, use one agent. Use several when you would naturally want a second pair of eyes or parallel hands.

Next: [02-architecture.md](02-architecture.md).
