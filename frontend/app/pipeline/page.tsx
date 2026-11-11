"use client";
import { useEffect, useState } from "react";
import { Alert, Badge, Empty, PageHead } from "../../components/ui";
import { api } from "../../lib/api";

function TaskSkills({ id, name, initial, onError }: { id: string; name: string; initial: string[]; onError: (m: string) => void }) {
  const [text, setText] = useState(initial.join(", "));
  const [note, setNote] = useState("");
  async function save() {
    onError("");
    setNote("");
    try {
      const r = await api(`/pipeline/tasks/${id}/skills`, "PUT", { skills: text.split(",").map((s) => s.trim()).filter(Boolean) });
      setNote(`Saved, about ${r.tokens.toLocaleString()} tokens`);
    } catch (e: any) {
      onError(e.message);
    }
  }
  return (
    <span className="row">
      <input aria-label={`Skills for ${name}`} name={`skills-${id}`} autoComplete="off" placeholder="skill names, comma separated" value={text} onChange={(e) => setText(e.target.value)} />
      <button type="button" className="secondary" onClick={save}>Save</button>
      {note && <span className="muted" role="status">{note}</span>}
    </span>
  );
}

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
      <PageHead title="Pipeline" sub="Each task runs in its own git worktree, then goes dev → test → review → merge." />
      <form onSubmit={start} className="card">
        <h2>New task</h2>
        <label htmlFor="pf">Approved feature</label>
        <select id="pf" name="feature" value={f.feature_id} onChange={(e) => setF({ ...f, feature_id: e.target.value })} required>
          <option value="">choose...</option>
          {features.map((x) => (
            <option key={x.id} value={x.id}>
              {x.title}
            </option>
          ))}
        </select>
        <label htmlFor="pn">Task name</label>
        <input id="pn" name="task_name" autoComplete="off" spellCheck={false} value={f.name} onChange={(e) => setF({ ...f, name: e.target.value })} pattern="[A-Za-z0-9][A-Za-z0-9._-]*" required />
        <div className="row">
          <button type="submit">Create Task Worktree</button>
        </div>
      </form>
      {err && <Alert>{err}</Alert>}
      <h2>Tasks</h2>
      {tasks.length === 0 ? (
        <Empty title="No tasks yet">Pick an approved feature above and create the first task worktree.</Empty>
      ) : (
        <div className="table-wrap">
          <table>
            <thead>
              <tr>
                <th scope="col">Task</th>
                <th scope="col">Branch</th>
                <th scope="col">State</th>
                <th scope="col">PR</th>
                <th scope="col">Tests</th>
                <th scope="col">Rounds</th>
                <th scope="col">Skills</th>
              </tr>
            </thead>
            <tbody>
              {tasks.map((t) => (
                <tr key={t.id}>
                  <td><strong>{t.name}</strong></td>
                  <td><code>{t.branch}</code></td>
                  <td><Badge status={t.status} /></td>
                  <td>{t.pr_number ? `#${t.pr_number}` : "–"}</td>
                  <td>{t.tests ? `${t.tests.passed} pass / ${t.tests.failed} fail` : "–"}</td>
                  <td>{t.rounds}</td>
                  <td>
                    <TaskSkills id={t.id} name={t.name} initial={t.skills ?? []} onError={setErr} />
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
      <h2>Merge queue</h2>
      <p className="muted">
        Approved PRs ({ready.length}) merge into the integration branch only if the merge gate passes. Merging into main stays with you.
      </p>
      <button disabled={ready.length === 0} onClick={mergeApproved}>
        Merge Approved Batch
      </button>
      {result && (
        <div className="card" role="status">
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
