"use client";
import { useEffect, useState } from "react";
import { runningCount } from "../lib/agents";
import { api } from "../lib/api";

const fmt = (n: number) => (n >= 1_000_000 ? `${(n / 1_000_000).toFixed(2)}M` : n >= 10_000 ? `${(n / 1000).toFixed(1)}k` : n.toLocaleString());

/** Live numbers for the Agents views: who is running right now and the token use, refreshed every 5 s from the app (no model is called). */
export default function LiveStrip({ rows, every = 5000 }: { rows: { state: string }[]; every?: number }) {
  const [daily, setDaily] = useState<{ used: number; limit: number | null } | null>(null);
  const [at, setAt] = useState<Date | null>(null);
  useEffect(() => {
    let live = true;
    const load = () =>
      api("/budget")
        .then((b) => {
          if (live) {
            setDaily(b.daily);
            setAt(new Date());
          }
        })
        .catch(() => {});
    load();
    const t = setInterval(load, every);
    return () => {
      live = false;
      clearInterval(t);
    };
  }, [every]);
  const active = runningCount(rows);
  return (
    <div className="live-strip" role="status" aria-label="Live agent numbers">
      <span><b>{active}</b> of {rows.length} working now</span>
      <span>Tokens, last 24 h: <b>{daily ? fmt(daily.used) : "–"}</b>{daily?.limit ? ` of ${fmt(daily.limit)}` : ""}</span>
      <span className="muted">Updates every {every / 1000} s{at ? ` · ${at.toLocaleTimeString()}` : ""}</span>
    </div>
  );
}
