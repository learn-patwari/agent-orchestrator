import { Block, parseMarkdown } from "./markdown";

export type StepState = "done" | "current" | "todo";
export type Step = { id: string; label: string; state: StepState };

const STEPS = [
  ["setup", "Folder, model and limit"],
  ["skills", "Skills"],
  ["questions", "Questions"],
  ["srs", "SRS"],
  ["build", "Build and release"],
] as const;

/** Which step a feature is on, from its status. Setup and skills have no status of their own: they are done once questions begin. */
const CURRENT: Record<string, string> = { questioning: "questions", ready: "questions", srs_draft: "srs", rejected: "srs", approved: "build" };

export function stepsFor(status: string): Step[] {
  const now = STEPS.findIndex(([id]) => id === (CURRENT[status] ?? "setup"));
  return STEPS.map(([id, label], i) => ({ id, label, state: i < now ? "done" : i === now ? "current" : "todo" }));
}

export type Section = { title: string; blocks: Block[] };

/** The SRS cut at its headings, so each part can be folded away and read on its own. */
export function sections(md: string): Section[] {
  const out: Section[] = [];
  for (const b of parseMarkdown(md)) {
    if (b.type === "heading" && b.level <= 2) out.push({ title: b.text, blocks: [] });
    else if (b.type === "heading") (out[out.length - 1] ?? out[out.push({ title: "", blocks: [] }) - 1]).blocks.push(b);
    else (out[out.length - 1] ?? out[out.push({ title: "", blocks: [] }) - 1]).blocks.push(b);
  }
  return out;
}
