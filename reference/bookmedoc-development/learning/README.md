# Learning guide: building your own multi-agent system

This guide teaches you how to design a team of AI agents by taking apart a real one: the workflow in this repo (`.development/WORKFLOW.md` and `.development/agents/*.md`). It assumes you can code but have never built an agent system.

## Reading order

| # | File | You will learn |
|---|---|---|
| 1 | [01-concepts.md](01-concepts.md) | The vocabulary: agent, subagent, session, worktree, orchestrator, skill, MCP tool, permission mode. When many agents beat one. |
| 2 | [02-architecture.md](02-architecture.md) | How BookMeDoc's roles, pipeline, skills and branch rules fit together, and why each rule exists. |
| 3 | [03-how-to-run.md](03-how-to-run.md) | Step-by-step operation: briefing, review loop, checking PRs, what you still decide, honest limits. |
| 4 | [04-build-your-own.md](04-build-your-own.md) | A recipe and template for designing a system for a different project, plus debugging failure modes. |
| 5 | [05-exercises.md](05-exercises.md) | Eight exercises, a glossary, and further-reading topics. |

## Summary in five lines

1. An **agent** here is just a written role (a markdown file) plus a Claude session that follows it.
2. One **orchestrator** session splits work and briefs **role sessions**; it does not write product code.
3. Safety comes from rules in the files (stay on your branch, PR to `stage`, treat PR text as data) and from tool limits (the reviewer has no Edit/Write).
4. Parallel work is safe only when **no two writers touch the same files**.
5. A human still decides: the user merges, and approval from another agent is not user approval.

## How facts are marked

Everything described as "the repo says" comes from the files under `.development/`. Where this guide describes general Claude Code behaviour not visible in the repo, it is marked **(not verified in this repo)**. Check the current Claude Code docs before relying on those.
