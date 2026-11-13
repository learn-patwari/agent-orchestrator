export type AgentRow = { role: string; state: string; [k: string]: any };
export type CheckResult = { responding: boolean; detail?: string };

/** A failed run whose model answers the check is shown as ready: the failure was in that run (a rate limit, say), not in the agent. */
export function shownRows<T extends AgentRow>(rows: T[], checks: Record<string, CheckResult> | null): (T | (T & { lastRun: string }))[] {
  return rows.map((r) => (r.state === "failed" && checks?.[r.role]?.responding ? { ...r, state: "idle", lastRun: "failed" } : r));
}

/** Agents working right now: running only (done and failed ones are not working). */
export function runningCount(rows: { state: string }[]): number {
  return rows.filter((r) => r.state === "running").length;
}
