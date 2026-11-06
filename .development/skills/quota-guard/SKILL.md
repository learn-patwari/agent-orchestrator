---
name: quota-guard
description: Reads platform limits (for example Vercel Hobby) and budgets pushes, merges and deploys against them. Use before any deploy, push-triggering merge or smoke run, and when a platform returns a limit error.
---

# Quota guard

- Know the limits table for the platform (Vercel Hobby: 100 deployments/day and 100/hour team-wide, 1 concurrent build, 45 min build, monthly transfer/invocation/CPU caps, non-commercial use only).
- Test locally first; push once per task; merge in batches (each merge into a deployed branch is a deployment); at most one read-only smoke run per batch, never on production URLs.
- Before any deploy or smoke run, count usage in the rolling window; stop at the threshold (default 70% of the limit) and tell the orchestrator, who tells the human.
- If the platform returns a limit error (for example `api-deployments-free-per-day`, 429), stop at once, report it, and do not retry; continue locally.
- Preview deployments (`target=null`) and production deployments are different things; show them separately.
