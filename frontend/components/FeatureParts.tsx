"use client";
import { useEffect, useState } from "react";
import { api } from "../lib/api";
import { Alert } from "./ui";

/** Read a SKILL.md the user picked: the name and description come from its header, the rest is the instructions. */
export function readSkillFile(text: string, fileName: string) {
  const m = /^---\r?\n([\s\S]*?)\r?\n---\r?\n?([\s\S]*)$/.exec(text);
  const get = (k: string) => new RegExp(`^${k}:\\s*(.*)$`, "m").exec(m?.[1] ?? "")?.[1]?.trim() ?? "";
  return { name: get("name") || fileName.replace(/\.md$/i, ""), description: get("description"), body: (m ? m[2] : text).trim() };
}

export function FeatureSkills({ fid }: { fid: string }) {
  const [d, setD] = useState<any>(null);
  const [pick, setPick] = useState<string[]>([]);
  const [err, setErr] = useState("");
  const [msg, setMsg] = useState("");
  const [up, setUp] = useState({ name: "", description: "", body: "" });

  async function load() {
    try {
      const x = await api<any>(`/features/${fid}/skills`);
      setD(x);
      setPick(x.selected);
    } catch (e: any) {
      setErr(e.detail ?? e.message);
    }
  }
  useEffect(() => {
    load();
  }, [fid]);

  async function save() {
    setErr("");
    setMsg("");
    try {
      const x = await api<any>(`/features/${fid}/skills`, "PUT", { skills: pick });
      setD(x);
      setPick(x.selected);
      setMsg("Skills saved.");
    } catch (e: any) {
      setErr(e.detail ?? e.message);
    }
  }
  async function upload(e: React.FormEvent) {
    e.preventDefault();
    setErr("");
    setMsg("");
    try {
      await api("/skills/upload", "POST", up);
      setPick((p) => [...p, up.name]);
      setUp({ name: "", description: "", body: "" });
      await load();
      setMsg("Skill added and ticked. Press Save skills to use it for this feature.");
    } catch (e: any) {
      setErr(e.detail ?? e.message);
    }
  }
  if (!d) return err ? <Alert>{err}</Alert> : null;
  const est = d.available.filter((s: any) => pick.includes(s.name)).reduce((n: number, s: any) => n + s.tokens, 0);
  return (
    <>
      <h3>Skills for this feature</h3>
      <p className="muted">Chosen skills are loaded into the agent's prompt when its run starts, so it does not spend steps looking for them. About {est.toLocaleString()} tokens per run.</p>
      {err && <Alert>{err}</Alert>}
      {msg && <Alert kind="ok">{msg}</Alert>}
      {d.available.length === 0 ? (
        <p className="muted">No skills yet. Add one below, or point Settings at a skills folder.</p>
      ) : (
        <ul className="plain">
          {d.available.map((s: any) => (
            <li key={s.name}>
              <label>
                <input type="checkbox" checked={pick.includes(s.name)} onChange={(e) => setPick(e.target.checked ? [...pick, s.name] : pick.filter((n) => n !== s.name))} /> {s.name}{" "}
                <span className="muted">~{s.tokens.toLocaleString()} tokens</span>
              </label>
              {pick.includes(s.name) && <p className="skill-desc">{s.description || "No description."}</p>}
            </li>
          ))}
        </ul>
      )}
      <button type="button" className="secondary" onClick={save}>
        Save skills
      </button>
      <details className="card">
        <summary>Need a skill that is not listed? Add one</summary>
        <form onSubmit={upload}>
          <label htmlFor="sk-f">Load a SKILL.md file (optional)</label>
          <input
            id="sk-f"
            type="file"
            accept=".md,text/markdown,text/plain"
            onChange={async (e) => {
              const f = e.target.files?.[0];
              if (f) setUp(readSkillFile(await f.text(), f.name));
            }}
          />
          <label htmlFor="sk-n">Skill name (letters, digits, - and _)</label>
          <input id="sk-n" autoComplete="off" value={up.name} onChange={(e) => setUp({ ...up, name: e.target.value })} required />
          <label htmlFor="sk-d">What it is for (one line)</label>
          <input id="sk-d" autoComplete="off" value={up.description} onChange={(e) => setUp({ ...up, description: e.target.value })} required />
          <label htmlFor="sk-b">Instructions</label>
          <textarea id="sk-b" rows={6} autoComplete="off" value={up.body} onChange={(e) => setUp({ ...up, body: e.target.value })} required />
          <button type="submit">Add skill</button>
        </form>
      </details>
    </>
  );
}

export function FeatureSetup({ fid }: { fid: string }) {
  const [setup, setSetup] = useState<any>(null);
  const [providers, setProviders] = useState<any[]>([]);
  const [graph, setGraph] = useState<any>(null);
  const [folder, setFolder] = useState("");
  const [pid, setPid] = useState("");
  const [model, setModel] = useState("");
  const [err, setErr] = useState("");
  const [msg, setMsg] = useState("");

  async function load() {
    try {
      const [s, p, g] = await Promise.all([api<any>(`/features/${fid}/setup`), api<any>("/providers"), api<any>(`/features/${fid}/graph`)]);
      setSetup(s);
      setProviders(Array.isArray(p) ? p : []);
      setGraph(g);
      setFolder(s.repo_path ?? "");
      setPid(s.model?.provider_id ?? "");
      setModel(s.model?.model ?? "");
    } catch (e: any) {
      setErr(e.detail ?? e.message);
    }
  }
  useEffect(() => {
    load();
  }, [fid]);

  async function save(e: React.FormEvent) {
    e.preventDefault();
    setErr("");
    setMsg("");
    try {
      setSetup(await api(`/features/${fid}/setup`, "PUT", { repo_path: folder, model: pid && model ? { provider_id: pid, model } : null }));
      setGraph(await api(`/features/${fid}/graph`));
      setMsg("Saved.");
    } catch (e: any) {
      setErr(e.detail ?? e.message);
    }
  }
  async function build() {
    setErr("");
    try {
      setGraph(await api(`/features/${fid}/graph/build`, "POST"));
    } catch (e: any) {
      setErr(e.detail ?? e.message);
    }
  }
  if (!setup) return err ? <Alert>{err}</Alert> : null;
  return (
    <>
      <h3>Code folder and model</h3>
      <p className="muted">Each feature can work in its own folder, with its own knowledge graph, and on its own model. Leave them empty to use the folder in Settings and each role's own model.</p>
      {err && <Alert>{err}</Alert>}
      {msg && <Alert kind="ok">{msg}</Alert>}
      <form onSubmit={save}>
        <label htmlFor="fs-f">Code folder for this feature</label>
        <input id="fs-f" autoComplete="off" placeholder="empty = the folder in Settings" value={folder} onChange={(e) => setFolder(e.target.value)} />
        <div className="row">
          <label htmlFor="fs-p">Provider</label>
          <select id="fs-p" value={pid} onChange={(e) => setPid(e.target.value)}>
            <option value="">Each role's own model</option>
            {providers.map((p) => (
              <option key={p.id} value={p.id}>{p.name}</option>
            ))}
          </select>
          <label htmlFor="fs-m">Model</label>
          <input id="fs-m" autoComplete="off" disabled={!pid} value={model} onChange={(e) => setModel(e.target.value)} />
        </div>
        <button type="submit">Save</button>
      </form>
      <h3>Knowledge graph</h3>
      <p className="muted" role="status">{graph?.built ? `Built: ${graph.files} files, ${graph.symbols} symbols.` : graph?.reason ?? ""}</p>
      <button type="button" className="secondary" onClick={build}>{graph?.built ? "Rebuild graph" : "Build graph"}</button>
    </>
  );
}
