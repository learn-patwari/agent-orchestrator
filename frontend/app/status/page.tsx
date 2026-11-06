"use client";
import { useEffect, useState } from "react";
import { Alert, Badge, Empty, PageHead } from "../../components/ui";
import { api } from "../../lib/api";

export default function Status() {
  const [rows, setRows] = useState<any[] | null>(null);
  const [err, setErr] = useState("");
  useEffect(() => {
    const load = () =>
      api("/status")
        .then((r) => {
          setRows(r);
          setErr("");
        })
        .catch((e) => setErr(e.message));
    load();
    const t = setInterval(load, 5000);
    return () => clearInterval(t);
  }, []);
  return (
    <>
      <PageHead title="Agents" sub="What each role is doing right now. Refreshes every 5 seconds." />
      {err && <Alert>{err}</Alert>}
      {rows && rows.length === 0 && <Empty title="No agents yet">Agents appear here once roles are loaded.</Empty>}
      {rows && rows.length > 0 && (
        <div className="table-wrap">
          <table>
            <thead>
              <tr>
                <th scope="col">Role</th>
                <th scope="col">State</th>
                <th scope="col">Task</th>
                <th scope="col">Steps</th>
              </tr>
            </thead>
            <tbody>
              {rows.map((r) => (
                <tr key={r.role}>
                  <td><strong>{r.role}</strong></td>
                  <td><Badge status={r.state} /></td>
                  <td>{r.task ?? "–"}</td>
                  <td>{r.steps}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </>
  );
}
