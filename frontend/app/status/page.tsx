"use client";
import { useEffect, useState } from "react";
import { Alert, Badge, Empty, PageHead } from "../../components/ui";
import { api } from "../../lib/api";

type TeamRow = {
  role: string;
  description: string;
  tools: string[];
  model: string | null;
  state: string;
  task: string | null;
  steps: number;
};
type CheckRow = { role: string; responding: boolean; detail: string };

export default function Status() {
  const [rows, setRows] = useState<TeamRow[] | null>(null);
  const [err, setErr] = useState("");
  const [checking, setChecking] = useState(false);
  const [checkErr, setCheckErr] = useState("");
  const [checks, setChecks] = useState<Record<string, CheckRow> | null>(null);

  useEffect(() => {
    const load = () =>
      api<TeamRow[]>("/team")
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
    setCheckErr("");
    try {
      const r = await api<CheckRow[]>("/team/check", "POST");
      setChecks(Object.fromEntries(r.map((c) => [c.role, c])));
    } catch (e: any) {
      setCheckErr(e.message || "The check failed.");
    } finally {
      setChecking(false);
    }
  }

  return (
    <>
      <PageHead title="Agents" sub="Each role, its model and what it is doing right now. Refreshes every 5 seconds.">
        <p>
          <button type="button" onClick={runCheck} disabled={checking || !rows || rows.length === 0} aria-busy={checking}>
            {checking ? "Checking…" : "Check agents"}
          </button>{" "}
          <span className="muted">Sends a short test message to each model, so it can take a minute.</span>
        </p>
      </PageHead>
      {err && <Alert>{err}</Alert>}
      {checkErr && <Alert>Check failed: {checkErr}</Alert>}
      {rows && rows.length === 0 && <Empty title="No agents yet">Agents appear here once roles are loaded.</Empty>}
      {rows && rows.length > 0 && (
        <div className="table-wrap">
          <table>
            <thead>
              <tr>
                <th scope="col">Role</th>
                <th scope="col">Description</th>
                <th scope="col">Model</th>
                <th scope="col">State</th>
                <th scope="col">Task</th>
                <th scope="col">Steps</th>
                {checks && <th scope="col">Check</th>}
              </tr>
            </thead>
            <tbody>
              {rows.map((r) => {
                const c = checks?.[r.role];
                return (
                  <tr key={r.role}>
                    <td className="nowrap"><strong>{r.role}</strong></td>
                    <td>{r.description || "–"}</td>
                    <td>{r.model ?? <Badge status="not set" tone="warn" />}</td>
                    <td><Badge status={r.state} /></td>
                    <td>{r.task ?? "–"}</td>
                    <td>{r.steps}</td>
                    {checks && (
                      <td>
                        {c ? (
                          <>
                            <Badge status={c.responding ? "Responding" : "Not responding"} tone={c.responding ? "ok" : "err"} />
                            {c.detail && <div className="muted">{c.detail}</div>}
                          </>
                        ) : (
                          "–"
                        )}
                      </td>
                    )}
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
