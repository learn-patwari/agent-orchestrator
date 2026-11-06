---
name: merge-gate
description: The reviewer's merge conditions as a checklist, with batching, stacked-PR order and head pinning. Use when deciding whether a PR may be merged into the integration branch, or when merging a batch.
---

# Merge gate

Merge into the integration branch only if ALL hold; otherwise report every failing reason:
1. Base is the integration branch (a stacked PR is retargeted after its base merges).
2. The latest review's first line is `Verdict: APPROVE` and its `Head:` line equals the PR's current head SHA.
3. No unresolved medium/high/critical finding in that review.
4. CI is success (no checks is not success) and the PR is mergeable.
5. Tests for the area ran, with counts, none failing.
6. The diff touches no secrets and no production/deployment config; and does not change reviewer permissions or policies.
7. Fewer than the round limit (default 3) of REQUEST_CHANGES reviews, else escalate to the human.

How: merge in batches, base PR first; re-read the head right before merging; pin it with `--match-head-commit`; use the configured style (merge commits by default); never `--admin`; never into main/master/stage (human only). Reviewer merge authority is a setting the human can switch off. A report is not proof: read the PR state, do not trust a message.
