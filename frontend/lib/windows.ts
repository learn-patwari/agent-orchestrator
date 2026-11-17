/** Usage window of a provider as typed in the form. `start` is a datetime-local value ("2026-01-01T09:00"), or empty for "now". */
export type WindowForm = { enabled: boolean; hours: string; start: string; ping: boolean; limit: string };
export const noWindow: WindowForm = { enabled: false, hours: "5", start: "", ping: true, limit: "" };

/** The `window` object the API expects, or undefined when the provider takes no part. OpenRouter has no windows. */
export function windowBody(w: WindowForm, preset: string): Record<string, unknown> | undefined {
  if (!w.enabled || preset === "openrouter") return undefined;
  const start = w.start ? new Date(w.start).getTime() / 1000 : undefined;
  const limit = parseInt(w.limit, 10);
  return { enabled: true, length_hours: parseFloat(w.hours) || 5, anchor: start, ping: w.ping, token_limit: limit > 0 ? limit : undefined };
}

export type Verdict = "match" | "differ" | "unknown";
/** Green when the provider's reset matches ours, red when it differs, grey when it reports none. */
export const verdictClass = (v: Verdict) => ({ match: "win-ok", differ: "win-bad", unknown: "win-none" })[v];
export const verdictText = (v: Verdict) => ({ match: "matches", differ: "differs", unknown: "not reported" })[v];

export const clock = (t?: number | null) => (t ? new Date(t * 1000).toLocaleString([], { weekday: "short", hour: "2-digit", minute: "2-digit" }) : "—");
