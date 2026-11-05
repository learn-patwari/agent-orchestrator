"use client";
import { useEffect, useState } from "react";
import { api } from "../../lib/api";

export default function Pipeline() {
  const [tasks, setTasks] = useState<any[]>([]);
  const [features, setFeatures] = useState<any[]>([]);
  const [f, setF] = useState({ feature_id: "", name: "" });
  const [result, setResult] = useState<any>(null);
  const [err, setErr] = useState("");

  async function load() {
    try {
      setTasks(await api("/pipeline/tasks"));
      setFeatures((await api("/features")).filter((x: any) => x.status === "approved"));
    } catch (e: any) {
      setErr(e.message);
    }
  }
  useEffect(() => {
    load();
  }, []);

  async function start(e: React.FormEvent) {
    e.preventDefault();
    setErr("");
    try {
      await api("/pipeline/tasks", "POST", f);
      setF({ ...f, name: "" });
      load();
    } catch (e: any) {
      setErr(e.message);
    }
  }
  async function mergeApproved() {
    setErr("");
    const numbers = tasks.filter((t) => t.status === "approved" && t.pr_number).map((t) => t.pr_number);
    try {
      setResult(await api("/pipeline/merge", "POST", { numbers }));
      load();
    } catch (e: any) {
      setErr(e.message);
    }
  }

  const ready = tasks.filter((t) => t.status === "approved" && t.pr_number);
  return (
    <>
      <h1>Pipeline</h1>
      <form onSubmit={start} className="card">
        <label htmlFor="pf">Approved feature</label>
        <select id="pf" value={f.feature_id} onChange={(e) => setF({ ...f, feature_id: e.target.value })} required>
          <option value="">choose...</option>
          {features.map((x) => (
            <option key={x.id} value={x.id}>
              {x.title}
            </option>
          ))}
        </select>
        <label htmlFor="pn">Task name</label>
        <input id="pn" value={f.name} onChange={(e) => setF({ ...f, name: e.target.value })} pattern="[A-Za-z0-9][A-Za-z0-9._-]*" required />
        <div className="row">
          <button type="submit">Create task worktree</button>
        </div>
      </form>
      {err && (
        <p className="err" role="alert">
          {err}
        </p>
      )}
      <h2>Tasks</h2>
      <table style={{ width: "100%", borderCollapse: "collapse" }}>
        <thead>
          <tr>
            <th align="left">Task</th>
            <th align="left">Branch</th>
            <th align="left">State</th>
            <th align="left">PR</th>
            <th align="left">Tests</th>
            <th align="left">Rounds</th>
          </tr>
        </thead>
        <tbody>
          {tasks.map((t) => (
            <tr key={t.id}>
              <td>{t.name}</td>
              <td>{t.branch}</td>
              <td>{t.status}</td>
              <td>{t.pr_number ?? "-"}</td>
              <td>{t.tests ? `${t.tests.passed} pass / ${t.tests.failed} fail` : "-"}</td>
              <td>{t.rounds}</td>
            </tr>
          ))}
        </tbody>
      </table>
      <h2>Merge queue</h2>
      <p className="muted">
        Approved PRs ({ready.length}) merge into the integration branch only if the merge gate passes. Merging into main stays with you.
      </p>
      <button disabled={ready.length === 0} onClick={mergeApproved}>
        Merge approved batch
      </button>
      {result && (
        <div className="card">
          <strong>Merged:</strong> {result.merged.map((m: any) => `#${m.number}`).join(", ") || "none"}
          {result.refused.map((r: any) => (
            <p key={r.number} className="err">
              #{r.number} refused: {r.reasons.join("; ")}
            </p>
          ))}
        </div>
      )}
    </>
  );
}
