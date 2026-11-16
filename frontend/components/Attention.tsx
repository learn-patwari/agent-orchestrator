"use client";
import { createContext, useContext, useEffect, useState, type ReactNode } from "react";
import { api } from "../lib/api";
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

/** A button with the total, opening a list. Each row goes to the exact place. */
export function AttentionBar() {
  const { items, seen } = useAttention();
  const [open, setOpen] = useState(false);
  const clearable = items.filter((i) => i.id.startsWith("run:")).map((i) => i.id);
  return (
    <div className="attn">
      <button type="button" className="secondary attn-btn" aria-expanded={open} aria-controls="attn-list" onClick={() => setOpen(!open)}>
        Waiting for you <Dot n={items.length} />
        {items.length === 0 && <span className="muted"> nothing</span>}
      </button>
      {open && (
        <div id="attn-list" className="attn-list card" role="region" aria-label="Waiting list">
          {items.length === 0 && <p className="muted">Nothing needs you right now.</p>}
          {items.map((i) => (
            <a key={i.id} href={i.link} className="attn-row" onClick={() => i.id.startsWith("run:") && seen([i.id])}>
              <strong>{i.title}</strong>
              <span className="muted">
                {actionWord(i.kind)}
                {i.detail ? ` — ${i.detail}` : ""}
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
