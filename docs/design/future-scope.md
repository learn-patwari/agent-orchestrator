# Future scope (user, 2026-11-08)

Three additions the user asked for. Nothing here is built yet. Each has a proposed shape, what it reuses, and the questions that need an answer before building.

## 1. Load skills for a task

**Ask.** For any feature or task the human can pick skills (specific technical knowledge). The agent starts with them already loaded, so it builds with fewer tokens and no time spent searching.

**Proposed shape.**
- A task or feature carries `skills: [...]`. When a run starts, the runner puts those skills' bodies straight into the system prompt. Skills that are not selected stay as a one-line catalog entry.
- Show an estimated token cost next to each skill, and the total, before the run starts.
- Reuses: the skill registry and `use_skill` (`backend/app/runtime/skills.py`), the role `skills` list.

**Questions.** Is it per feature, per run, or both? Should a skill that is too big for the budget be summarised or refused?

## 2. Agents that announce themselves (registry, like Eureka)

**Ask.** Load any external agent. When it comes up it signals the Orchestrator ("I am up, I can take tasks like these"), and the Orchestrator picks it up and understands what it is for.

**Proposed shape.**
- `POST /api/registry/register` with name, endpoint, capabilities, skills and the protocol it speaks; a heartbeat with a time-to-live. No heartbeat means the agent drops out of the list.
- The Orchestrator's team snapshot lists registered agents next to the built-in roles, with their capabilities and whether they are live. `delegate` can target a registered agent over its endpoint.
- Safety: a registration token, and a new agent is **pending until the human approves it**. Anything an external agent says is untrusted data (same rule as every tool result). Delegating outside the machine stays behind the existing external-providers feature flag (D-2).
- Reuses: the provider interface that already allows external providers behind a flag, the inbox for the approval, the office view (a registered agent gets a desk when live).

**Questions.** Which protocol: plain HTTP with our own schema, MCP, or an agent-to-agent standard? Does a registered agent run on this machine or elsewhere?

## 3. Token optimisation, limits and resume after a window

**Ask.** Anyone can choose and change the architecture to use fewer tokens. Show the token limit everywhere. The human sets how many tokens a feature may use; when it is reached the work **holds**; when the human says the limit window has reset (for example the five-hour Claude window), development continues.

**Proposed shape.**
- **Limits.** A token limit per feature (and optionally per run, per day). Shown on the feature, the run, the Agents page and in chat. The Orchestrator is told the remaining budget in its context.
- **Hold and resume.** At the limit new runs for that feature are refused and in-flight runs stop at the next step; the feature goes to `held_budget`. The human can set a resume time ("continue at 18:30") or press Resume; a scheduled job continues the held work. The app cannot read your provider's quota, so the window is something you tell it, not something it detects.
- **Architecture profiles.** A named set of settings: compaction threshold, how many skills are preloaded, model per role (cheap model for simple roles), brief size, step limit. Ship a few (for example "lean", "balanced", "thorough"); the human can edit one or save their own; the active one is shown with its estimated cost.
- Reuses: budgets and usage records (`budgets`, `llm_calls`), run limits (`run_limits`), the scheduler (`schedules`), context compaction in the runner, the inbox for "held, needs you".

**Questions.** Is the limit counted in tokens only, or tokens and cost? One fixed window length, or entered each time? What happens to a half-finished step when a hold starts: finish it, or stop at once?

## Suggested order
1. Token limits with hold and resume (it protects every other piece).
2. Skills loaded per task (small, and lowers cost straight away).
3. Architecture profiles.
4. Agent registry.
