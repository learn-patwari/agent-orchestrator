/** One thing that waits for the human: an approval, a question, or a run that finished or failed. */
export type AttentionItem = { id: string; kind: "approval" | "question" | "task_done" | "task_failed"; title: string; detail: string; link: string; at: number };

/** The sidebar tab each item belongs to, by where its link goes. */
export function tabFor(link: string): string {
  if (link.startsWith("/features")) return "/features";
  if (link.startsWith("/inbox")) return "/inbox";
  if (link.startsWith("/status")) return "/status";
  return "/";
}

/** How many items wait on each sidebar tab (the red numbers). */
export function tabCounts(items: AttentionItem[]): Record<string, number> {
  const out: Record<string, number> = {};
  for (const i of items) {
    const t = tabFor(i.link);
    out[t] = (out[t] ?? 0) + 1;
  }
  return out;
}

/** What a human should do with the item, in a few words. */
export function actionWord(kind: AttentionItem["kind"]): string {
  return { approval: "Approve or reject", question: "Answer", task_done: "Review", task_failed: "Look at the failure" }[kind];
}
