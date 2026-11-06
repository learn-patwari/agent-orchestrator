"use client";
import { useEffect, useState } from "react";
import { Alert, Badge, Empty, PageHead } from "../../components/ui";
import { api } from "../../lib/api";

type Check = { role: string; responding: boolean; detail: string };

export default function Status() {
  const [rows, setRows] = useState<any[] | null>(null);
  const [err, setErr] = useState("");
  const [checks, setChecks] = useState<Record<string, Check> | null>(null);
  const [checking, setChecking] = useState(false);

  useEffect(() => {
    const load = () =>
      api("/team")
        .then((r) => {
          setRows(r);
          setErr("");
        })
        .catch((e) => setErr(e.message));
    load();
    const t = setInterval(load, 5000);
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

  const answered = checks ? Object.values(checks).filter((c) => c.responding).length : 0;
  return (
    <>
      <PageHead title="Agents" sub="Each role, what it does, which model it uses and what it is doing now. Refreshes every 5 seconds." />
      <div className="row">
        <button onClick={runCheck} disabled={checking || !rows?.length}>
          {checking ? "Checking…" : "Check Agents"}
        </button>
        <span className="muted">Asks each model once whether it answers. It uses a little of your quota, so it only runs when you click.</span>
      </div>
      {checking && <p className="muted" role="status">Checking each model. This can take up to a minute…</p>}
      {checks && !checking && (
        <Alert kind={answered === Object.keys(checks).length ? "ok" : "warn"}>
          {answered} of {Object.keys(checks).length} roles have a model that answered.
        </Alert>
      )}
      {err && <Alert>{err}</Alert>}
      {rows && rows.length === 0 && <Empty title="No agents yet">Agents appear here once roles are loaded.</Empty>}
      {rows && rows.length > 0 && (
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
