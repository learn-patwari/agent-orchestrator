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

function TokenLimit({ fid }: { fid: string }) {
  const [b, setB] = useState<any>(null);
  const [limit, setLimit] = useState("");
  const [at, setAt] = useState("");
  const [err, setErr] = useState("");

  async function load() {
    try {
      const d = await api(`/features/${fid}/budget`);
      setB(d);
      setLimit(d.limit ? String(d.limit) : "");
      setAt(d.resume_at ? new Date(d.resume_at * 1000).toISOString().slice(0, 16) : "");
    } catch (e: any) {
      setErr(e.message);
    }
  }
  useEffect(() => {
    load();
  }, [fid]);

  async function save(e: React.FormEvent) {
    e.preventDefault();
    setErr("");
    try {
      const resume = at ? new Date(at + "Z").getTime() / 1000 : null;
      setB(await api(`/features/${fid}/budget`, "PUT", { limit: limit ? Number(limit) : null, resume_at: resume }));
    } catch (e: any) {
      setErr(e.message);
    }
  }
  async function resume() {
    setErr("");
    try {
      setB(await api(`/features/${fid}/resume`, "POST"));
    } catch (e: any) {
      setErr(e.message);
    }
  }
  if (!b) return err ? <Alert>{err}</Alert> : null;
  return (
    <>
      <h3>Token limit</h3>
      <p className="muted">
        {b.limit ? `${b.used.toLocaleString()} of ${b.limit.toLocaleString()} tokens used` : `${b.used.toLocaleString()} tokens used, no limit set`}
      </p>
      {b.held && (
        <Alert kind="warn">
          Held at the limit. Work continues when you press Resume{b.resume_at ? ` or at ${new Date(b.resume_at * 1000).toLocaleString()}` : ""}.
        </Alert>
      )}
      {err && <Alert>{err}</Alert>}
      <form onSubmit={save} className="row">
        <label htmlFor="tl">Limit (tokens)</label>
        <input id="tl" name="limit" type="number" min={1} inputMode="numeric" autoComplete="off" placeholder="no limit" value={limit} onChange={(e) => setLimit(e.target.value)} />
        <label htmlFor="ta">Resume at (UTC, optional)</label>
        <input id="ta" name="resume_at" type="datetime-local" autoComplete="off" value={at} onChange={(e) => setAt(e.target.value)} />
        <button type="submit">Save limit</button>
        {b.held && (
          <button type="button" className="secondary" onClick={resume}>
            Resume now
          </button>
        )}
      </form>
    </>
  );
}

function FeatureSkills({ fid }: { fid: string }) {
  const [d, setD] = useState<any>(null);
  const [pick, setPick] = useState<string[]>([]);
  const [err, setErr] = useState("");

  useEffect(() => {
    api(`/features/${fid}/skills`)
      .then((x) => {
        setD(x);
        setPick(x.selected);
      })
      .catch((e) => setErr(e.message));
  }, [fid]);

  async function save() {
    setErr("");
    try {
      const x = await api(`/features/${fid}/skills`, "PUT", { skills: pick });
      setD(x);
      setPick(x.selected);
    } catch (e: any) {
      setErr(e.message);
    }
  }
  if (!d) return err ? <Alert>{err}</Alert> : null;
  const est = d.available.filter((s: any) => pick.includes(s.name)).reduce((n: number, s: any) => n + s.tokens, 0);
  return (
    <>
      <h3>Skills for this feature</h3>
      <p className="muted">Chosen skills are loaded into the agent's prompt when its run starts, so it does not spend steps looking for them. About {est.toLocaleString()} tokens per run.</p>
      {err && <Alert>{err}</Alert>}
      {d.available.length === 0 ? (
        <p className="muted">No skills found. Add a skills folder in Settings.</p>
      ) : (
        <ul className="plain">
          {d.available.map((s: any) => (
            <li key={s.name}>
              <label>
                <input type="checkbox" checked={pick.includes(s.name)} onChange={(e) => setPick(e.target.checked ? [...pick, s.name] : pick.filter((n) => n !== s.name))} />{" "}
                {s.name} <span className="muted">~{s.tokens.toLocaleString()} tokens. {s.description}</span>
              </label>
            </li>
          ))}
        </ul>
      )}
      <button type="button" className="secondary" onClick={save}>
        Save skills
      </button>
    </>
  );
}

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
        <ul className="chips" aria-label="Features">
          {list.map((x) => (
            <li key={x.id}>
              <button className="secondary chip" aria-pressed={sel?.id === x.id} onClick={() => load(x.id)}>
                <span>{x.title}</span> <Badge status={x.status} />
              </button>
            </li>
          ))}
        </ul>
      )}
      {sel && (
        <div className="card">
          <h2>{sel.title}</h2>
          <p>
            <strong>Next:</strong> {STEP[sel.status] ?? sel.status}{" "}
            {(sel.status === "questioning" || sel.status === "srs_draft") && <Link href="/inbox">Open inbox</Link>}
          </p>
          {sel.status !== "rejected" && <TokenLimit key={sel.id} fid={sel.id} />}
          {sel.status !== "rejected" && <FeatureSkills key={`s${sel.id}`} fid={sel.id} />}
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
