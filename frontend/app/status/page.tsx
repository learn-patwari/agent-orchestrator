"use client";
import { useEffect, useState } from "react";
import LiveStrip from "../../components/LiveStrip";
import Office from "../../components/Office";
import AgentState from "../../components/AgentState";
import { Alert, Badge, Empty, PageHead } from "../../components/ui";
import { api } from "../../lib/api";
import { CHAR_STYLES, CharStyle, Environments, Handoff, seatLabel } from "../../lib/office-scene";
import { useOffice } from "../../lib/use-office";

type Check = { role: string; responding: boolean; detail: string };
type View = "office" | "table";
type Pending = { id: string; name: string; approvals: string[] };
const NO_ENV: Environments = { qc: null, prod: null };

export default function Status() {
  const { data, err: feedErr, setData } = useOffice();
  const rows: any[] | null = data ? data.team : null;
  const handoffs: Handoff[] = data?.handoffs ?? [];
  const env: Environments = data?.environments ?? NO_ENV;
  const pending: Pending[] = data?.pending_prod ?? [];
  const incidents: number = data?.open_incidents ?? 0;
  const docs: { id: string; name: string; status: string }[] = data?.docs_gates ?? [];
  const win = data?.usage_window ?? null;
  const [budget, setBudget] = useState<any>(null);
  const [actErr, setErr] = useState("");
  const err = actErr || feedErr;
  const [checks, setChecks] = useState<Record<string, Check> | null>(null);
  const [checking, setChecking] = useState(false);
  const [view, setView] = useState<View>("office");
  const [paused, setPaused] = useState(false);
  const [selected, setSelected] = useState<string | null>(null);
  const [look, setLook] = useState<CharStyle>("robots");

  useEffect(() => {
    try {
      const v = localStorage.getItem("ao.office.look") as CharStyle | null;
      if (v && CHAR_STYLES.some((c) => c.id === v)) setLook(v);
    } catch {}
  }, []);
  function pickLook(v: CharStyle) {
    setLook(v);
    try { localStorage.setItem("ao.office.look", v); } catch {}
  }

  useEffect(() => {
    if (window.innerWidth < 640) setView("table"); // the office needs room; phones get the table
  }, []);

  useEffect(() => {
    api("/budget")
      .then(setBudget)
      .catch(() => {});
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
      setData(await api("/office"));
    } catch (e: any) {
      setErr(e.message);
    }
  }

  const answered = checks ? Object.values(checks).filter((c) => c.responding).length : 0;
  const sel = rows?.find((r) => r.role === selected);
  return (
    <>
      <PageHead title="Agents" sub="Your team at work. Click anyone for details." />
      <div className="toolbar">
        <div className="seg" role="group" aria-label="View">
          <button className="secondary" aria-pressed={view === "office"} onClick={() => setView("office")}>Office</button>
          <button className="secondary" aria-pressed={view === "table"} onClick={() => setView("table")}>Table</button>
        </div>
        {view === "office" && (
          <button className="secondary" onClick={() => setPaused(!paused)} aria-pressed={paused}>
            {paused ? "Play" : "Pause"}
          </button>
        )}
        {view === "office" && (
          <label className="look">
            <span className="muted">Characters</span>
            <select value={look} onChange={(e) => pickLook(e.target.value as CharStyle)}>
              {CHAR_STYLES.map((c) => <option key={c.id} value={c.id}>{c.label}</option>)}
            </select>
          </label>
        )}
        <span className="spacer" />
        {view === "office" && (
          <button className="secondary" onClick={() => window.open("/agents-window/", "agents-window", "popup,width=1280,height=820")} title="Open the office in its own window, for a second monitor.">
            Detach
          </button>
        )}
        <button onClick={runCheck} disabled={checking || !rows?.length} title="Asks each model once whether it answers. Uses a little quota, so it only runs when you click.">
          {checking ? "Checking…" : "Check agents"}
        </button>
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
      {win?.blocked && (
        <Alert kind="warn">
          Usage window is at {win.percent}%. New work is stopped and running work is held until it resets
          {win.resets_at ? ` at ${new Date(win.resets_at * 1000).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}` : ""}; it then continues by itself.
        </Alert>
      )}
      {docs.filter((d) => d.status === "FAIL" || d.status === "MISSING").map((d) => (
        <Alert key={d.id} kind="warn">
          “{d.name}” is waiting for the docs gate. The technical writer fixes the documents, then ci/cd runs the check again.
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
        <div className="office-layout">
          <div className="office-main">
          <LiveStrip rows={rows} />
          <Office team={rows} handoffs={handoffs} environments={env} paused={paused} look={look} onSelect={setSelected} />
          </div>
          <div className="office-side">
          <div className="agent-chips" aria-label="Agent status">
            {rows.map((r) => (
              <button key={r.role} className="agent-chip" onClick={() => setSelected(r.role)} aria-pressed={selected === r.role}>
                {seatLabel(r.role)} <AgentState state={r.state} />
                {checks?.[r.role] && <small title={checks[r.role].detail}>{checks[r.role].responding ? "model answers" : "no answer"}</small>}
              </button>
            ))}
          </div>
          <p className="muted caption">The colour is the agent&apos;s last run. “Model answers” is the Check agents result: a failed run can still have a working model, for example after a rate limit.</p>
          <div className="card" aria-live="polite">
            {sel ? (
              <>
                <strong>{seatLabel(sel.role)}</strong> <AgentState state={sel.state} />
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
          </div>
        </div>
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
                    <td><AgentState state={r.state} /></td>
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
