"use client";
import { createContext, useContext, useEffect, useState, type ReactNode } from "react";
import { api } from "../lib/api";
import { short } from "../lib/activity";
import { actionWord, tabCounts, type AttentionItem } from "../lib/attention";

type Ctx = { items: AttentionItem[]; counts: Record<string, number>; seen: (ids: string[]) => Promise<void> };
const Attn = createContext<Ctx>({ items: [], counts: {}, seen: async () => {} });
export const useAttention = () => useContext(Attn);

/** One poll of the attention feed (every 5 s) shared by the sidebar dots and the bell bar. */
export function AttentionProvider({ children }: { children: ReactNode }) {
  const [items, setItems] = useState<AttentionItem[]>([]);
  const load = () =>
    api<{ items: AttentionItem[] }>("/attention")
      .then((r) => setItems(r.items ?? []))
      .catch(() => {}); // signed out or offline: show nothing rather than an error
  useEffect(() => {
    load();
    const t = setInterval(load, 5000);
    return () => clearInterval(t);
  }, []);
  const seen = async (ids: string[]) => {
    try {
      await api("/attention/seen", "POST", { ids });
    } catch {}
    load();
  };
  return <Attn.Provider value={{ items, counts: tabCounts(items), seen }}>{children}</Attn.Provider>;
}

/** The red dot with a number. Renders nothing at zero. */
export function Dot({ n }: { n?: number }) {
  if (!n) return null;
  return (
    <span className="attn-dot" role="status" aria-label={`${n} waiting`}>
      {n > 9 ? "9+" : n}
    </span>
  );
}

/** The bell in the header's right corner: a red number when something waits, a list under it. Each row goes to the exact place. */
export function AttentionBar() {
  const { items, seen } = useAttention();
  const [open, setOpen] = useState(false);
  const clearable = items.filter((i) => i.id.startsWith("run:")).map((i) => i.id);
  return (
    <div className="attn">
      <button type="button" className="secondary attn-btn" aria-label={items.length ? `Notifications, ${items.length} waiting` : "Notifications, nothing waiting"} aria-expanded={open} aria-controls="attn-list" onClick={() => setOpen(!open)}>
        <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
          <path d="M6 8a6 6 0 0 1 12 0c0 7 3 9 3 9H3s3-2 3-9" />
          <path d="M10.3 21a1.94 1.94 0 0 0 3.4 0" />
        </svg>
        <Dot n={items.length} />
      </button>
      {open && (
        <div id="attn-list" className="attn-list card" role="region" aria-label="Waiting list">
          <strong className="attn-head">Waiting for you</strong>
          {items.length === 0 && <p className="muted">Nothing needs you right now.</p>}
          {items.map((i) => (
            <a key={i.id} href={i.link} className="attn-row" title={i.title} onClick={() => i.id.startsWith("run:") && seen([i.id])}>
              <strong className="clamp-2">{short(i.title, 80)}</strong>
              <span className="muted clamp-2">
                {actionWord(i.kind)}
                {i.detail ? ` — ${short(i.detail, 100)}` : ""}
              </span>
            </a>
          ))}
          {clearable.length > 0 && (
            <button type="button" className="secondary" onClick={() => seen(clearable)}>
              Clear finished and failed
            </button>
          )}
        </div>
      )}
    </div>
  );
}
