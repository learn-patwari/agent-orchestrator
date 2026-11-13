"use client";
import Link from "next/link";
import { useEffect, useState } from "react";
import { api } from "../lib/api";

type Daily = { limit: number | null; used: number; percent: number | null };
type Win = { percent: number | null; resets_at: number | null; blocked: boolean };
type Feature = { id: string; title: string; limit: number | null; used: number; held: boolean };
type Graph = { built: boolean; files?: number; tokens_saved: number };
type Snapshot = { daily: Daily; features: Feature[]; win: Win | null; graph: Graph | null };

const fmt = (n: number) => (n >= 1_000_000 ? `${(n / 1_000_000).toFixed(2)}M` : n >= 10_000 ? `${(n / 1000).toFixed(1)}k` : n.toLocaleString());

/** Plain-language state for the meter: colour is never the only signal. */
export function level(percent: number | null, blocked = false): "none" | "ok" | "warn" | "stop" {
  if (blocked) return "stop";
  if (percent === null) return "none";
  return percent >= 100 ? "stop" : percent >= 80 ? "warn" : "ok";
}

/** Shown at the top of every page: tokens used against the budget, the provider window, and held features. */
export default function TokenBar() {
  const [s, setS] = useState<Snapshot | null>(null);

  useEffect(() => {
    let live = true;
    const load = async () => {
      if (document.hidden) return;
      try {
        const [b, w, g] = await Promise.all([api("/budget"), api("/usage-window").catch(() => null), api("/graph").catch(() => null)]);
        if (live) setS({ daily: b.daily, features: b.features ?? [], win: w, graph: g });
      } catch {
        if (live) setS(null); // signed out or the backend is down: show nothing rather than a wrong number
      }
    };
    load();
    const t = setInterval(load, 8000);
    document.addEventListener("visibilitychange", load);
    return () => {
      live = false;
      clearInterval(t);
      document.removeEventListener("visibilitychange", load);
    };
  }, []);

  if (!s) return null;
  const { daily, features, win, graph } = s;
  const held = features.filter((f) => f.held);
  const state = level(daily.percent, win?.blocked ?? false);
  const pct = Math.min(100, daily.percent ?? 0);
  const word = { none: "no daily limit", ok: "within budget", warn: "nearly used up", stop: "limit reached, work is held" }[state];

  return (
    <div className={`tokenbar tb-${state}`} role="region" aria-label="Token usage">
      <div className="tb-main">
        <span className="tb-label">Tokens, last 24 h</span>
        <strong>{fmt(daily.used)}</strong>
        {daily.limit ? <span className="tb-of">of {fmt(daily.limit)}</span> : null}
        {daily.limit ? (
          <div className="tb-meter" role="progressbar" aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(pct)} aria-label="Daily token budget used">
            <span style={{ width: `${pct}%` }} />
          </div>
        ) : null}
        <span className="tb-state">{daily.percent !== null ? `${daily.percent}% · ${word}` : word}</span>
      </div>
      <div className="tb-extra">
        {win?.percent !== null && win?.percent !== undefined ? <span>Provider window {win.percent}%{win.blocked ? " (full)" : ""}</span> : null}
        {graph?.built ? (
          <span title="Estimated tokens not spent reading whole files, because agents asked the knowledge graph instead">
            Graph: {graph.files?.toLocaleString()} files, ~{fmt(graph.tokens_saved)} tokens saved
          </span>
        ) : null}
        {held.length > 0 ? <span className="tb-held">{held.length} feature{held.length > 1 ? "s" : ""} held</span> : null}
        <Link href="/profiles">{daily.limit ? "Budget" : "Set a budget"}</Link>
      </div>
    </div>
  );
}
