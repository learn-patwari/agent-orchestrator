export type Run = { id: string; role: string; task: string; status: string; steps?: number; tokens?: number; feature_id?: string; error?: string | null };
export type Task = { id: string; name: string; status: string; feature_id?: string; owner?: string };

/** The first line of a task text, cut at a word so it fits a list row. */
export function short(text: string, max = 90): string {
  const t = (text ?? "").replace(/\s+/g, " ").trim();
  if (t.length <= max) return t;
  const cut = t.slice(0, max);
  return cut.slice(0, Math.max(cut.lastIndexOf(" "), 40)).trimEnd() + "…";
}

/** What is happening for one feature: who is working now, the latest finished runs, its tasks, and one plain sentence. */
export function activityFor(fid: string, runs: Run[], tasks: Task[]) {
  const mine = runs.filter((r) => r.feature_id === fid);
  const working = mine.filter((r) => r.status === "running");
  const recent = mine.filter((r) => r.status !== "running").slice(-5).reverse();
  const myTasks = tasks.filter((t) => t.feature_id === fid);
  let headline: string;
  if (working.length) headline = `${working.length} agent${working.length > 1 ? "s" : ""} working now: ${[...new Set(working.map((r) => r.role))].join(", ")}.`;
  else if (myTasks.length) headline = "No agent is running right now. The tasks below are waiting for the next step.";
  else if (mine.length) headline = "No agent is running right now. Nothing has been handed to a developer yet.";
  else headline = "Nothing has started for this feature yet.";
  return { working, recent, tasks: myTasks, headline, idle: working.length === 0 };
}
