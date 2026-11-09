"use client";
import { useEffect, useState } from "react";
import { Alert, Badge, PageHead } from "../../components/ui";
import { api } from "../../lib/api";

const FIELDS: [string, string][] = [
  ["max_steps", "Steps per run"],
  ["max_total_tokens", "Total tokens per run"],
  ["max_output_tokens", "Output tokens per call"],
  ["compact_at_chars", "Compact the context at (characters)"],
  ["keep_recent", "Recent messages kept after compaction"],
];

export default function Profiles() {
  const [d, setD] = useState<any>(null);
  const [err, setErr] = useState("");
  const [name, setName] = useState("");
  const [vals, setVals] = useState<Record<string, string>>({});

  async function load() {
    try {
      setD(await api("/profiles"));
    } catch (e: any) {
      setErr(e.message);
    }
  }
  useEffect(() => {
    load();
  }, []);

  const [daily, setDaily] = useState("");
  const [dailyMsg, setDailyMsg] = useState("");
  useEffect(() => {
    api("/settings")
      .then((s) => setDaily(s.daily_token_limit ? String(s.daily_token_limit) : ""))
      .catch(() => {});
  }, []);

  async function saveDaily(e: React.FormEvent) {
    e.preventDefault();
    setErr("");
    try {
      await api("/settings", "PUT", { daily_token_limit: daily ? Number(daily) : null });
      setDailyMsg(daily ? `Daily limit set to ${Number(daily).toLocaleString()} tokens.` : "Daily limit removed.");
    } catch (e: any) {
      setErr(e.message);
    }
  }

  const [wPct, setWPct] = useState("");
  const [wReset, setWReset] = useState("");
  const [wMsg, setWMsg] = useState("");
  useEffect(() => {
    api("/usage-window")
      .then((w) => {
        if (w.percent != null) setWPct(String(w.percent));
        if (w.resets_at) setWReset(new Date(w.resets_at * 1000 - new Date().getTimezoneOffset() * 60000).toISOString().slice(0, 16));
      })
      .catch(() => {});
  }, []);

  async function saveWindow(e: React.FormEvent) {
    e.preventDefault();
    setErr("");
    try {
      const w = await api("/usage-window", "PUT", {
        percent: Number(wPct),
        resets_at: wReset ? new Date(wReset).getTime() / 1000 : null,
      });
      setWMsg(w.blocked ? "Window is full: new work is stopped until the reset." : "Saved. Work can continue.");
    } catch (e: any) {
      setErr(e.message);
    }
  }

  async function act(fn: () => Promise<any>) {
    setErr("");
    try {
      setD(await fn());
    } catch (e: any) {
      setErr(e.message);
    }
  }

  function startFrom(p: any) {
    setName(p.builtin ? `${p.name}-copy` : p.name);
    setVals(Object.fromEntries(FIELDS.map(([k]) => [k, String(p[k])])));
  }

  function save(e: React.FormEvent) {
    e.preventDefault();
    const body = Object.fromEntries(FIELDS.map(([k]) => [k, Number(vals[k])]));
    act(() => api(`/profiles/${encodeURIComponent(name.trim())}`, "PUT", body));
  }

  return (
    <>
      <PageHead title="Token profiles" sub="How much an agent run may spend. The active profile applies to every new run. Pick one, or copy one and change it." />
      {err && <Alert>{err}</Alert>}
      <form onSubmit={saveDaily} className="card">
        <h2>Daily token limit</h2>
        <p className="muted">Tokens used across all work in any rolling 24 hours. At the limit new runs are refused and running work holds, then continues by itself when the window has room. Leave empty for no limit.</p>
        <label htmlFor="dl">Tokens per 24 hours</label>
        <input id="dl" name="daily" type="number" min={1} inputMode="numeric" autoComplete="off" placeholder="no limit" value={daily} onChange={(e) => setDaily(e.target.value)} />
        <div className="row">
          <button type="submit">Save daily limit</button>
        </div>
        {dailyMsg && <Alert kind="ok">{dailyMsg}</Alert>}
      </form>
      <form onSubmit={saveWindow} className="card">
        <h2>Usage window guard</h2>
        <p className="muted">The app cannot read your provider&apos;s 5-hour window, so enter how full it is and when it resets. At 98% new work stops, running work holds, and one resume is scheduled for the reset.</p>
        <label htmlFor="wp">Percent used</label>
        <input id="wp" name="percent" type="number" min={0} max={100} step="any" inputMode="decimal" autoComplete="off" required value={wPct} onChange={(e) => setWPct(e.target.value)} />
        <label htmlFor="wr">Resets at</label>
        <input id="wr" name="resets" type="datetime-local" autoComplete="off" value={wReset} onChange={(e) => setWReset(e.target.value)} />
        <div className="row">
          <button type="submit">Save usage window</button>
        </div>
        {wMsg && <Alert kind="ok">{wMsg}</Alert>}
      </form>
      {d && (
        <ul className="plain" aria-label="Profiles">
          {d.profiles.map((p: any) => (
            <li key={p.name} className="card">
              <h2>
                {p.name} {p.name === d.active && <Badge status="active" />} {p.builtin && <span className="muted">built in</span>}
              </h2>
              <p className="muted">
                Up to {p.est_run_tokens.toLocaleString()} tokens and {p.max_steps} steps per run, {p.max_output_tokens} output tokens per call, compacts at {p.compact_at_chars.toLocaleString()} characters.
              </p>
              <div className="row">
                {p.name !== d.active && (
                  <button type="button" onClick={() => act(() => api("/profiles/active", "POST", { name: p.name }))}>
                    Use this profile
                  </button>
                )}
                <button type="button" className="secondary" onClick={() => startFrom(p)}>
                  {p.builtin ? "Copy to edit" : "Edit"}
                </button>
                {!p.builtin && (
                  <button type="button" className="secondary" onClick={() => act(() => api(`/profiles/${encodeURIComponent(p.name)}`, "DELETE"))}>
                    Delete
                  </button>
                )}
              </div>
            </li>
          ))}
        </ul>
      )}
      <form onSubmit={save} className="card">
        <h2>Your own profile</h2>
        <label htmlFor="pn">Name</label>
        <input id="pn" name="name" autoComplete="off" value={name} onChange={(e) => setName(e.target.value)} required />
        {FIELDS.map(([k, label]) => (
          <div key={k}>
            <label htmlFor={`pf-${k}`}>{label}</label>
            <input id={`pf-${k}`} name={k} type="number" min={1} inputMode="numeric" autoComplete="off" value={vals[k] ?? ""} onChange={(e) => setVals({ ...vals, [k]: e.target.value })} required />
          </div>
        ))}
        <div className="row">
          <button type="submit">Save profile</button>
        </div>
      </form>
    </>
  );
}
