"use client";
import { useEffect, useState } from "react";
import { Alert, Badge, Empty, PageHead } from "../../components/ui";
import { api } from "../../lib/api";

export default function Plugins() {
  const [d, setD] = useState<{ installed: any[]; available: any[] } | null>(null);
  const [path, setPath] = useState("");
  const [err, setErr] = useState("");
  const [a, setA] = useState({ name: "", description: "", instructions: "", tools: "read_file, list_dir", skills: "" });
  const [built, setBuilt] = useState("");

  async function build(e: React.FormEvent) {
    e.preventDefault();
    setErr("");
    setBuilt("");
    const list = (t: string) => t.split(",").map((x) => x.trim()).filter(Boolean);
    try {
      const r = await api("/agents/build", "POST", { ...a, tools: list(a.tools), skills: list(a.skills) });
      setBuilt(`Built as plugin ${r.plugin}. Enable it below to use the role.`);
      setA({ name: "", description: "", instructions: "", tools: "read_file, list_dir", skills: "" });
      await load();
    } catch (e: any) {
      setErr(e.message);
    }
  }

  async function load() {
    try {
      setD(await api("/plugins"));
    } catch (e: any) {
      setErr(e.message);
    }
  }
  useEffect(() => {
    load();
  }, []);

  async function act(fn: () => Promise<any>) {
    setErr("");
    try {
      await fn();
    } catch (e: any) {
      setErr(e.message);
    }
    await load();
  }

  const installed = new Set((d?.installed ?? []).map((p) => p.name));
  return (
    <>
      <PageHead title="Plugins" sub="A plugin is a folder of roles, skills, workflows and templates. It is checked when installed, and one with problems stays off until it is fixed." />
      {err && <Alert>{err}</Alert>}
      <h2>Ships with the app</h2>
      {(d?.available ?? []).length === 0 ? (
        <Empty title="Nothing ships with this build">Install a plugin from a folder below.</Empty>
      ) : (
        <ul className="plain" aria-label="Available plugins">
          {d!.available.map((p) => (
            <li key={p.name} className="card">
              <h3>
                {p.name} <span className="muted">{p.version}</span>
              </h3>
              <p>{p.description}</p>
              <div className="row">
                <button type="button" disabled={installed.has(p.name)} onClick={() => act(() => api("/plugins/install", "POST", { path: p.path }))}>
                  {installed.has(p.name) ? "Installed" : "Install"}
                </button>
              </div>
            </li>
          ))}
        </ul>
      )}
      <form
        className="card"
        onSubmit={(e) => {
          e.preventDefault();
          act(() => api("/plugins/install", "POST", { path: path.trim() })).then(() => setPath(""));
        }}
      >
        <h2>Install from a folder</h2>
        <label htmlFor="pp">Folder on this machine (it contains plugin.json)</label>
        <input id="pp" name="path" autoComplete="off" placeholder="C:\plugins\my-plugin" value={path} onChange={(e) => setPath(e.target.value)} required />
        <div className="row">
          <button type="submit">Install</button>
        </div>
      </form>
      <form onSubmit={build} className="card">
        <h2>Build an agent</h2>
        <p className="muted">Describe a role. It is checked, then installed as a plugin you can enable, export and share.</p>
        <label htmlFor="an">Name (lowercase, no spaces)</label>
        <input id="an" name="name" autoComplete="off" value={a.name} onChange={(e) => setA({ ...a, name: e.target.value })} required />
        <label htmlFor="ad">What it does</label>
        <input id="ad" name="description" autoComplete="off" value={a.description} onChange={(e) => setA({ ...a, description: e.target.value })} required />
        <label htmlFor="ai">Instructions</label>
        <textarea id="ai" name="instructions" rows={5} autoComplete="off" value={a.instructions} onChange={(e) => setA({ ...a, instructions: e.target.value })} required />
        <label htmlFor="at">Tools it may use (comma separated)</label>
        <input id="at" name="tools" autoComplete="off" value={a.tools} onChange={(e) => setA({ ...a, tools: e.target.value })} />
        <label htmlFor="as">Skills (comma separated, optional)</label>
        <input id="as" name="skills" autoComplete="off" value={a.skills} onChange={(e) => setA({ ...a, skills: e.target.value })} />
        <div className="row">
          <button type="submit">Build agent</button>
        </div>
        {built && <Alert kind="ok">{built}</Alert>}
      </form>
      <h2>Installed</h2>
      {(d?.installed ?? []).length === 0 ? (
        <Empty title="No plugins installed">Install one above.</Empty>
      ) : (
        <ul className="plain" aria-label="Installed plugins">
          {d!.installed.map((p) => (
            <li key={p.name} className="card">
              <h3>
                {p.name} <span className="muted">{p.version}</span> <Badge status={p.enabled ? "enabled" : "disabled"} />
              </h3>
              <p>{p.description}</p>
              {p.problems?.length > 0 && (
                <Alert kind="warn">
                  Problems found, so it stays off:
                  <ul>
                    {p.problems.map((x: string) => (
                      <li key={x}>{x}</li>
                    ))}
                  </ul>
                </Alert>
              )}
              <div className="row">
                {p.enabled ? (
                  <button type="button" className="secondary" onClick={() => act(() => api(`/plugins/${p.name}/disable`, "POST"))}>
                    Disable
                  </button>
                ) : (
                  <button type="button" onClick={() => act(() => api(`/plugins/${p.name}/enable`, "POST"))}>
                    Enable
                  </button>
                )}
                <button type="button" className="secondary" onClick={() => { window.location.href = `/api/plugins/${p.name}/export`; }}>
                  Export zip
                </button>
                <button type="button" className="secondary" onClick={() => act(() => api(`/plugins/${p.name}`, "DELETE"))}>
                  Remove
                </button>
              </div>
            </li>
          ))}
        </ul>
      )}
    </>
  );
}
