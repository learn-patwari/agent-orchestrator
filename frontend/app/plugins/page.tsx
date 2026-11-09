"use client";
import { useEffect, useState } from "react";
import { Alert, Badge, Empty, PageHead } from "../../components/ui";
import { api } from "../../lib/api";

export default function Plugins() {
  const [d, setD] = useState<{ installed: any[]; available: any[] } | null>(null);
  const [path, setPath] = useState("");
  const [err, setErr] = useState("");

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
