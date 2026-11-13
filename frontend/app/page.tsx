"use client";
import Link from "next/link";
import { useEffect, useState } from "react";
import { Alert, Badge, PageHead } from "../components/ui";
import { runningCount } from "../lib/agents";
import { api } from "../lib/api";

type State = "loading" | "setup" | "login" | "in";
type Counts = { providers: number; rolesSet: number; roles: number; features: number; open: number; agentsBusy: number };

export default function Home() {
  const [state, setState] = useState<State>("loading");
  const [pw, setPw] = useState("");
  const [err, setErr] = useState("");
  const [busy, setBusy] = useState(false);
  const [c, setC] = useState<Counts | null>(null);

  async function loadCounts() {
    const [p, r, f, i, s] = await Promise.allSettled([
      api<any[]>("/providers"), api("/role-models"), api<any[]>("/features"), api<any[]>("/inbox?status=open"), api<any[]>("/status"),
    ]);
    const ok = <T,>(x: PromiseSettledResult<T>) => (x.status === "fulfilled" ? x.value : null);
    const roles = ok(r);
    setC({
      providers: ok(p)?.length ?? 0, roles: roles?.roles.length ?? 0, rolesSet: roles ? Object.keys(roles.models).length : 0,
      features: ok(f)?.length ?? 0, open: ok(i)?.length ?? 0, agentsBusy: runningCount(ok(s) ?? []),
    });
  }

  async function refresh() {
    const s = await api<{ setup_done: boolean }>("/auth/status");
    try {
      await api("/settings");
      setState("in");
      loadCounts();
    } catch {
      setState(s.setup_done ? "login" : "setup");
    }
  }
  useEffect(() => {
    refresh().catch((e) => setErr(e.message));
  }, []);

  async function submit(kind: "setup" | "login") {
    setErr("");
    setBusy(true);
    try {
      await api(`/auth/${kind}`, "POST", { password: pw });
      setPw("");
      await refresh();
    } catch (e: any) {
      setErr(e.message);
    } finally {
      setBusy(false);
    }
  }

  if (state === "loading") return err ? <Alert>{err}</Alert> : <p className="muted" role="status">Loading…</p>;

  if (state === "in") {
    const steps = [
      { done: (c?.providers ?? 0) > 0, title: "Add an LLM provider", text: "Pick OpenAI, Anthropic, Gemini, Foundry, Ollama, your Claude login, or a custom endpoint.", href: "/providers", cta: "Open Providers" },
      { done: !!c && c.roles > 0 && c.rolesSet >= c.roles, title: "Choose a model for each role", text: "The Orchestrator and every agent need a provider and model.", href: "/providers", cta: "Assign models" },
      { done: (c?.features ?? 0) > 0, title: "Describe your first feature", text: "Tell the Orchestrator what to build. It asks questions, then writes the SRS for you to approve.", href: "/chat", cta: "Start in Chat" },
    ];
    const next = steps.findIndex((s) => !s.done);
    return (
      <>
        <PageHead title="Welcome back" sub="Your team of AI agents, with the Orchestrator as the only voice to you." />
        <div className="grid stats" role="group" aria-label="Summary">
          <div className="card stat"><b>{c?.features ?? "–"}</b><span>Features</span></div>
          <div className="card stat"><b>{c?.open ?? "–"}</b><span>Open items in Inbox</span></div>
          <div className="card stat"><b>{c?.agentsBusy ?? "–"}</b><span>Agents working now</span></div>
          <div className="card stat"><b>{c ? `${c.rolesSet}/${c.roles}` : "–"}</b><span>Roles with a model</span></div>
        </div>
        <h2>Get started</h2>
        {steps.map((s, i) => (
          <div className="card" key={s.title} style={{ display: "flex", gap: 14, alignItems: "center", flexWrap: "wrap" }}>
            <div style={{ flex: "1 1 260px", minWidth: 0 }}>
              <strong>{i + 1}. {s.title}</strong> <Badge status={s.done ? "done" : i === next ? "next" : "pending"} tone={s.done ? "ok" : i === next ? "info" : ""} />
              <p className="muted" style={{ margin: "4px 0 0" }}>{s.text}</p>
            </div>
            <Link href={s.href} className={i === next ? "btn" : "btn secondary"}>
              {s.cta}
            </Link>
          </div>
        ))}
        <div className="row" style={{ marginTop: 24 }}>
          <button
            className="secondary"
            onClick={async () => {
              await api("/auth/logout", "POST");
              refresh();
            }}
          >
            Sign out
          </button>
        </div>
      </>
    );
  }

  return (
    <div className="auth">
      <form
        className="card"
        onSubmit={(e) => {
          e.preventDefault();
          submit(state);
        }}
      >
        <h1>{state === "setup" ? "Create your password" : "Sign in"}</h1>
        <p className="muted">{state === "setup" ? "First run. This password protects your orchestrator on this computer." : "Enter your password to continue."}</p>
        <label htmlFor="pw">Password</label>
        <input
          id="pw" name="password" type="password" value={pw} onChange={(e) => setPw(e.target.value)} minLength={8} required
          autoComplete={state === "setup" ? "new-password" : "current-password"} autoFocus
        />
        {state === "setup" && <p className="field-note">At least 8 characters.</p>}
        <div className="row">
          <button type="submit" disabled={busy}>{busy ? "Working…" : state === "setup" ? "Create Password" : "Sign In"}</button>
        </div>
        {err && <Alert>{err}</Alert>}
      </form>
    </div>
  );
}
