"use client";
import { useEffect, useState } from "react";
import Office from "../../components/Office";
import { Alert, Badge, Empty, PageHead } from "../../components/ui";
import { api } from "../../lib/api";
import { Environments, Handoff, STATUS_COLORS, seatLabel, toStatus } from "../../lib/office-scene";

type Check = { role: string; responding: boolean; detail: string };
type View = "office" | "table";
type Pending = { id: string; name: string; approvals: string[] };
const NO_ENV: Environments = { qc: null, prod: null };

export default function Status() {
  const [rows, setRows] = useState<any[] | null>(null);
  const [handoffs, setHandoffs] = useState<Handoff[]>([]);
  const [env, setEnv] = useState<Environments>(NO_ENV);
  const [pending, setPending] = useState<Pending[]>([]);
  const [incidents, setIncidents] = useState(0);
  const [budget, setBudget] = useState<any>(null);
  const [err, setErr] = useState("");
  const [checks, setChecks] = useState<Record<string, Check> | null>(null);
  const [checking, setChecking] = useState(false);
  const [view, setView] = useState<View>("office");
  const [paused, setPaused] = useState(false);
  const [selected, setSelected] = useState<string | null>(null);

  useEffect(() => {
    if (window.innerWidth < 640) setView("table"); // the office needs room; phones get the table
  }, []);

  useEffect(() => {
    const load = () =>
      api("/office")
        .then((r) => {
          setRows(r.team);
          setHandoffs(r.handoffs);
          setEnv(r.environments ?? NO_ENV);
          setPending(r.pending_prod ?? []);
          setIncidents(r.open_incidents ?? 0);
          setErr("");
        })
        .catch((e) => setErr(e.message));
    api("/budget")
      .then(setBudget)
      .catch(() => {});
    load();
    const t = setInterval(load, 4000);
    return () => clearInterval(t);
  }, []);

  async function runCheck() {
    setChecking(true);
    setErr("");
    try {
      const r: Check[] = await api("/team/check", "POST");
      setChecks(Object.fromEntries(r.map((c) => [c.role, c])));
    } catch (e: any) {
      setErr(e.message);
    } finally {
      setChecking(false);
    }
  }

  async function approveProd(id: string) {
    setErr("");
    try {
      await api(`/pipeline/tasks/${id}/approve-prod`, "POST", { by: "human" });
      const r = await api("/office");
      setPending(r.pending_prod ?? []);
      setEnv(r.environments ?? NO_ENV);
    } catch (e: any) {
      setErr(e.message);
    }
  }

  const answered = checks ? Object.values(checks).filter((c) => c.responding).length : 0;
  const sel = rows?.find((r) => r.role === selected);
  return (
    <>
      <PageHead title="Agents" sub="Each role, what it does, which model it uses and what it is doing now. Refreshes every 4 seconds." />
      <div className="row">
        <button onClick={runCheck} disabled={checking || !rows?.length}>
          {checking ? "Checking…" : "Check Agents"}
        </button>
        <button onClick={() => setView(view === "office" ? "table" : "office")} aria-pressed={view === "table"}>
          {view === "office" ? "Show table" : "Show office"}
        </button>
        {view === "office" && (
          <button onClick={() => setPaused(!paused)} aria-pressed={paused}>
            {paused ? "Play" : "Pause"}
          </button>
        )}
        <span className="muted">Check Agents asks each model once whether it answers. It uses a little of your quota, so it only runs when you click.</span>
      </div>
      {checking && <p className="muted" role="status">Checking each model. This can take up to a minute…</p>}
      {checks && !checking && (
        <Alert kind={answered === Object.keys(checks).length ? "ok" : "warn"}>
          {answered} of {Object.keys(checks).length} roles have a model that answered.
        </Alert>
      )}
      {err && <Alert>{err}</Alert>}
      {budget && (budget.daily.limit || budget.features.some((f: any) => f.limit)) && (
        <p className="muted" role="status">
          Tokens, last 24 hours: {budget.daily.used.toLocaleString()}
          {budget.daily.limit ? ` of ${budget.daily.limit.toLocaleString()}` : " (no daily limit)"}.{" "}
          {budget.features
            .filter((f: any) => f.limit)
            .map((f: any) => `${f.title}: ${f.used.toLocaleString()} of ${f.limit.toLocaleString()}${f.held ? " (held)" : ""}`)
            .join("; ")}
        </p>
      )}
      {budget?.features.filter((f: any) => f.held).map((f: any) => (
        <Alert key={f.id} kind="warn">
          “{f.title}” is held at its token limit. Resume it on the Features page once your window has cleared.
        </Alert>
      ))}
      {incidents > 0 && (
        <Alert kind="warn">
          {incidents} open production {incidents === 1 ? "incident is" : "incidents are"} with production support.
        </Alert>
      )}
      {pending.map((p) => (
        <Alert key={p.id} kind="warn">
          <strong>Production needs your yes:</strong> deploy “{p.name}”? QC has signed off.{" "}
          {p.approvals.includes("human") ? (
            <span>You approved. Waiting for the architect&apos;s sign-off.</span>
          ) : (
            <button onClick={() => approveProd(p.id)}>Approve production deploy</button>
          )}
        </Alert>
      ))}
      {rows && rows.length === 0 && <Empty title="No agents yet">Agents appear here once roles are loaded.</Empty>}

      {rows && rows.length > 0 && view === "office" && (
        <>
          <Office team={rows} handoffs={handoffs} environments={env} paused={paused} onSelect={setSelected} />
          <div className="agent-chips" aria-label="Agent status">
            {rows.map((r) => (
              <button key={r.role} className="agent-chip" onClick={() => setSelected(r.role)} aria-pressed={selected === r.role}>
                <i className="dot" style={{ background: STATUS_COLORS[toStatus(r.state)] }} />
                {seatLabel(r.role)}
                <small>{r.state}</small>
              </button>
            ))}
          </div>
          <p className="muted" style={{ marginTop: 8 }}>
            Everyone stays at their desk. The office boy walks only when the app records a real hand-off: a delegation, a review, a deploy or a run reporting back. The QC and PROD racks light up with a version only when something is deployed there. The receptionist and the cat are just for fun.
          </p>
          <div className="card" aria-live="polite">
            {sel ? (
              <>
                <strong>{seatLabel(sel.role)}</strong> <Badge status={sel.state} />
                <dl className="detail">
                  <dt>Role</dt><dd>{sel.description || "–"}</dd>
                  <dt>Task</dt><dd>{sel.task ?? "–"}</dd>
                  <dt>Model</dt><dd>{sel.model ?? "not set"}</dd>
                  <dt>Steps</dt><dd>{sel.steps}</dd>
                </dl>
              </>
            ) : (
              <span className="muted">Click a person, or a status button, to see its role, task and model.</span>
            )}
          </div>
        </>
      )}

      {rows && rows.length > 0 && view === "table" && (
        <div className="table-wrap">
          <table>
            <thead>
              <tr>
                <th scope="col">Role</th>
                <th scope="col">Model</th>
                <th scope="col">State</th>
                <th scope="col">Task</th>
                <th scope="col">Steps</th>
                <th scope="col">Responding</th>
              </tr>
            </thead>
            <tbody>
              {rows.map((r) => {
                const c = checks?.[r.role];
                return (
                  <tr key={r.role}>
                    <td>
                      <strong>{r.role}</strong>
                      <div className="muted" style={{ fontSize: "0.85rem" }}>{r.description}</div>
                    </td>
                    <td>{r.model ?? <Badge status="not set" tone="warn" />}</td>
                    <td><Badge status={r.state} /></td>
                    <td>{r.task ?? "–"}</td>
                    <td>{r.steps}</td>
                    <td>
                      {c ? (
                        <>
                          <Badge status={c.responding ? "responding" : "not responding"} tone={c.responding ? "ok" : "err"} />
                          {!c.responding && <div className="muted" style={{ fontSize: "0.82rem" }}>{c.detail}</div>}
                        </>
                      ) : (
                        <span className="muted">not checked</span>
                      )}
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}
    </>
  );
}
