"use client";
import Link from "next/link";
import { useEffect, useState } from "react";
import { Alert, Badge, Empty, PageHead } from "../../components/ui";
import { api } from "../../lib/api";

const STEP: Record<string, string> = {
  questioning: "Answer the questions in the Inbox",
  ready: "All questions answered: generate the SRS",
  srs_draft: "SRS ready: approve it in the Inbox to start development",
  approved: "Approved: development can start",
  rejected: "SRS rejected",
};

export default function Features() {
  const [list, setList] = useState<any[]>([]);
  const [sel, setSel] = useState<any>(null);
  const [srs, setSrs] = useState("");
  const [f, setF] = useState({ title: "", text: "" });
  const [err, setErr] = useState("");
  const [busy, setBusy] = useState(false);

  async function load(id?: string) {
    try {
      setList(await api("/features"));
      const cur = id ?? sel?.id;
      if (cur) {
        const d = await api(`/features/${cur}`);
        setSel(d);
        setSrs(d.srs_md ?? "");
      }
    } catch (e: any) {
      setErr(e.message);
    }
  }
  useEffect(() => {
    load();
  }, []);

  async function run(fn: () => Promise<any>) {
    setErr("");
    setBusy(true);
    try {
      return await fn();
    } catch (e: any) {
      setErr(e.message);
    } finally {
      setBusy(false);
    }
  }

  async function create(e: React.FormEvent) {
    e.preventDefault();
    const d = await run(() => api("/features", "POST", f));
    if (d) {
      setF({ title: "", text: "" });
      await load(d.id);
    }
  }

  return (
    <>
      <PageHead title="Features" sub="Describe what you want. The architect asks questions, then writes the SRS and a phased plan for you to approve." />
      <form onSubmit={create} className="card">
        <label htmlFor="ft">Title</label>
        <input id="ft" name="title" autoComplete="off" value={f.title} onChange={(e) => setF({ ...f, title: e.target.value })} required />
        <label htmlFor="fx">What do you want?</label>
        <textarea id="fx" name="text" autoComplete="off" rows={5} placeholder="e.g. Let patients book, move and cancel appointments online…" value={f.text} onChange={(e) => setF({ ...f, text: e.target.value })} required />
        <div className="row">
          <button type="submit" disabled={busy}>
            {busy ? "Analysing…" : "Hand to the Architect"}
          </button>
        </div>
      </form>
      {err && <Alert>{err}</Alert>}
      {list.length === 0 ? (
        <Empty title="No features yet">Your first feature will appear here once you hand it to the architect.</Empty>
      ) : (
        <div className="row" role="list" aria-label="Features">
          {list.map((x) => (
            <button key={x.id} role="listitem" className="secondary" aria-pressed={sel?.id === x.id} onClick={() => load(x.id)}>
              {x.title} <Badge status={x.status} />
            </button>
          ))}
        </div>
      )}
      {sel && (
        <div className="card">
          <h2>{sel.title}</h2>
          <p>
            <strong>Next:</strong> {STEP[sel.status] ?? sel.status}{" "}
            {(sel.status === "questioning" || sel.status === "srs_draft") && <Link href="/inbox">Open inbox</Link>}
          </p>
          <h3>Understanding</h3>
          <p>{sel.analysis?.restatement}</p>
          <p className="muted">{sel.analysis?.why}</p>
          <h3>Impact (architect view)</h3>
          <ul>
            {["modules", "data", "security", "risks"].map((k) => (
              <li key={k}>
                <strong>{k}:</strong> {(sel.analysis?.impact?.[k] ?? []).join(", ") || "none identified"}
              </li>
            ))}
          </ul>
          <h3>Decisions and assumptions</h3>
          <ul>
            {sel.qa.map((q: any) => (
              <li key={q.item_id}>
                {q.question}: <em>{q.answer}</em> {q.by !== "human" && <strong>[A]</strong>}
              </li>
            ))}
          </ul>
          {sel.status === "ready" && (
            <button disabled={busy} onClick={async () => { await run(() => api(`/features/${sel.id}/finalize`, "POST")); await load(sel.id); }}>
              {busy ? "Writing SRS…" : "Generate SRS and Plan"}
            </button>
          )}
          {sel.plan && (
            <>
              <h3>Plan (lanes run in parallel; same lane = stacked)</h3>
              {sel.plan.map((ph: any) => (
                <div key={ph.id}>
                  <strong>{ph.id}: {ph.name}</strong>
                  <ul>
                    {ph.tasks.map((t: any) => (
                      <li key={t.id}>
                        {t.id} {t.title} <span className="muted">lane {t.lane}, wave {t.wave}
                        {t.stacked_on ? `, stacked on ${t.stacked_on}` : ""}</span>
                      </li>
                    ))}
                  </ul>
                </div>
              ))}
            </>
          )}
          {srs && (
            <>
              <h3>SRS</h3>
              <pre style={{ whiteSpace: "pre-wrap" }}>{srs}</pre>
            </>
          )}
        </div>
      )}
    </>
  );
}
