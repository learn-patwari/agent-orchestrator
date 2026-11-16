"use client";
import { useEffect, useRef, useState } from "react";
import { Alert, Badge, Empty, PageHead } from "../../components/ui";
import { api } from "../../lib/api";

type Result = { state: "testing" | "ok" | "fail"; text: string };

export default function Providers() {
  const [list, setList] = useState<any[]>([]);
  const [types, setTypes] = useState<string[]>([]);
  const [presets, setPresets] = useState<any[]>([]);
  const [pid, setPid] = useState("openai");
  const seeded = useRef(false);
  const [roles, setRoles] = useState<string[]>([]);
  const [assigned, setAssigned] = useState<Record<string, { provider_id: string; model: string }>>({});
  const [rp, setRp] = useState("");
  const [rm, setRm] = useState("");
  const [f, setF] = useState({ name: "", type: "openai", base_url: "", api_key: "" });
  const [models, setModels] = useState<Record<string, string>>({});
  const [res, setRes] = useState<Record<string, Result>>({});
  const [err, setErr] = useState("");
  const [saved, setSaved] = useState("");
  const [loaded, setLoaded] = useState(false);

  async function load() {
    try {
      setList(await api("/providers"));
      setTypes((await api("/providers/types")).types);
      const ps = (await api("/providers/presets")).presets;
      setPresets(ps);
      if (!seeded.current && ps.length) {
        seeded.current = true; // apply the preselected provider once, so its URL is not left empty
        const first = ps.find((x: any) => x.id === "openai") ?? ps[0];
        setF((cur) => ({ ...cur, type: first.type, base_url: first.base_url, name: cur.name || first.name }));
      }
      const rmod = await api("/role-models");
      setRoles(rmod.roles);
      setAssigned(rmod.models);
    } catch (e: any) {
      setErr(e.message);
    } finally {
      setLoaded(true);
    }
  }
  useEffect(() => {
    load();
  }, []);

  async function add(e: React.FormEvent) {
    e.preventDefault();
    setErr("");
    try {
      await api("/providers", "POST", f);
      setSaved(`Added ${f.name || "provider"}. Test it below, then assign it to roles.`);
      setF({ ...f, name: "", api_key: "" });
      load();
    } catch (e: any) {
      setErr(e.message);
    }
  }
  async function test(id: string) {
    setRes((x) => ({ ...x, [id]: { state: "testing", text: "Testing… this can take up to a minute" } }));
    let r: Result;
    try {
      const t = await api(`/providers/${id}/test`, "POST", { model: models[id] });
      r = t.ok
        ? { state: "ok", text: `Works (${t.usage.input_tokens} in / ${t.usage.output_tokens} out tokens)` }
        : { state: "fail", text: `Failed (${t.error_kind}): ${t.error}` };
    } catch (e: any) {
      r = { state: "fail", text: `Failed: ${e.message}` };
    }
    setRes((x) => ({ ...x, [id]: r }));
  }
  async function remove(p: any) {
    if (!window.confirm(`Delete provider “${p.name}”? Roles that use it will need a new provider.`)) return;
    try {
      await api(`/providers/${p.id}`, "DELETE");
      load();
    } catch (e: any) {
      setErr(e.message);
    }
  }
  async function assign(rolesToSet: string[]) {
    setErr("");
    try {
      for (const r of rolesToSet) await api(`/role-models/${r}`, "PUT", { provider_id: rp, model: rm });
      setSaved(rolesToSet.length > 1 ? "Model set for all roles." : `Model set for ${rolesToSet[0]}.`);
      load();
    } catch (e: any) {
      setErr(e.message);
    }
  }
  const preset = presets.find((p) => p.id === pid);
  const cli = f.type === "claude_cli";
  const needsKey = !cli && (preset ? preset.needs_key : true);
  function choose(id: string) {
    setPid(id);
    const p = presets.find((x) => x.id === id);
    if (p) setF({ ...f, type: p.type, base_url: p.base_url, name: f.name || p.name });
  }
  const rolesSet = roles.filter((r) => assigned[r]).length;

  return (
    <>
      <PageHead title="Providers" sub="Connect the language models your agents use, then choose which model each role gets." />
      {err && <Alert>{err}</Alert>}
      {saved && !err && <Alert kind="ok">{saved}</Alert>}

      <h2>1. Connected providers</h2>
      {loaded && list.length === 0 ? (
        <Empty title="No provider yet">Add one below. No API key? Use “Claude (your own login)” or a local Ollama.</Empty>
      ) : (
        <div className="grid">
          {list.map((p) => {
            const r = res[p.id];
            return (
              <div className="card" key={p.id}>
                <div className="row" style={{ justifyContent: "space-between", margin: 0 }}>
                  <strong style={{ overflowWrap: "anywhere" }}>{p.name}</strong>
                  <Badge status={r ? (r.state === "ok" ? "works" : r.state === "testing" ? "testing" : "failed") : "not tested"} tone={r ? (r.state === "ok" ? "ok" : r.state === "testing" ? "info" : "err") : ""} />
                </div>
                <p className="muted" style={{ margin: "4px 0 0", overflowWrap: "anywhere" }}>
                  {p.type === "claude_cli" ? "Your Claude login" : p.type} · {p.base_url || "auto-detect"}
                  {p.type !== "claude_cli" && ` · key ${p.api_key || "none"}`}
                </p>
                <label htmlFor={`m-${p.id}`}>Model to test</label>
                <div className="row" style={{ margin: 0 }}>
                  <input
                    id={`m-${p.id}`} name="model" autoComplete="off" spellCheck={false} style={{ flex: "1 1 140px" }}
                    placeholder={p.type === "claude_cli" ? "sonnet" : "e.g. gpt-4o…"}
                    value={models[p.id] ?? ""} onChange={(e) => setModels({ ...models, [p.id]: e.target.value })}
                  />
                  <button className="secondary" disabled={!models[p.id] || r?.state === "testing"} onClick={() => test(p.id)}>
                    {r?.state === "testing" ? "Testing…" : "Test Connection"}
                  </button>
                </div>
                {r && (
                  <p className={r.state === "fail" ? "err" : "muted"} role={r.state === "fail" ? "alert" : "status"} style={{ margin: "8px 0 0", overflowWrap: "anywhere" }}>
                    {r.text}
                  </p>
                )}
                <div className="row" style={{ marginBottom: 0 }}>
                  <button className="danger small" onClick={() => remove(p)}>Delete</button>
                </div>
              </div>
            );
          })}
        </div>
      )}

      <h2>2. Add a provider</h2>
      <form onSubmit={add} className="card">
        <label htmlFor="pr">Provider</label>
        <select id="pr" value={pid} onChange={(e) => choose(e.target.value)}>
          {presets.map((p) => (
            <option key={p.id} value={p.id}>
              {p.name}
            </option>
          ))}
        </select>
        {preset?.note && <p className="field-note">{preset.note}</p>}
        {pid === "custom" && (
          <>
            <label htmlFor="t">Protocol</label>
            <select id="t" value={f.type} onChange={(e) => setF({ ...f, type: e.target.value })}>
              {types.filter((t) => t !== "claude_cli").map((t) => (
                <option key={t}>{t}</option>
              ))}
            </select>
          </>
        )}
        <label htmlFor="n">Name</label>
        <input id="n" name="name" autoComplete="off" value={f.name} onChange={(e) => setF({ ...f, name: e.target.value })} required />
        <label htmlFor="u">{cli ? "Path to claude.exe (optional)" : "Base URL"}</label>
        <input
          id="u" name="base_url" type={cli ? "text" : "url"} inputMode="url" autoComplete="off" spellCheck={false}
          placeholder={cli ? "Leave empty to find it automatically…" : "https://api.example.com/v1"}
          value={f.base_url} onChange={(e) => setF({ ...f, base_url: e.target.value })} required={!cli}
        />
        {!cli && (
          <>
            <label htmlFor="k">API key{needsKey ? "" : " (not needed)"}</label>
            <input id="k" name="api_key" type="password" autoComplete="off" spellCheck={false} value={f.api_key} onChange={(e) => setF({ ...f, api_key: e.target.value })} required={needsKey} />
            <p className="field-note">Stored encrypted on this computer and never shown again in full.</p>
          </>
        )}
        <div className="row">
          <button type="submit">Add Provider</button>
        </div>
      </form>

      <h2>3. Models for roles <span className="muted" style={{ fontWeight: 400 }}>({rolesSet} of {roles.length} set)</span></h2>
      <p className="muted">The Orchestrator and every agent need a provider and model before chat works. There is no separate Save: a role marked “saved” below is already stored. To change one, pick a provider and model here, then press Use for All Roles or that row&apos;s Use Selection.</p>
      <div className="card">
        <div className="grid" style={{ gridTemplateColumns: "repeat(auto-fit, minmax(220px, 1fr))" }}>
          <div>
            <label htmlFor="rp" style={{ marginTop: 0 }}>Provider</label>
            <select id="rp" value={rp} onChange={(e) => setRp(e.target.value)}>
              <option value="">Choose…</option>
              {list.map((p) => (
                <option key={p.id} value={p.id}>
                  {p.name}
                </option>
              ))}
            </select>
          </div>
          <div>
            <label htmlFor="rm" style={{ marginTop: 0 }}>Model</label>
            <input id="rm" name="role_model" autoComplete="off" spellCheck={false} value={rm} onChange={(e) => setRm(e.target.value)} placeholder="e.g. sonnet, gpt-4o, gemini-2.0-flash…" />
          </div>
        </div>
        <div className="row">
          <button disabled={!rp || !rm} onClick={() => assign(roles)}>
            Use for All Roles
          </button>
        </div>
        <div className="table-wrap" style={{ boxShadow: "none", marginTop: 8 }}>
          <table>
            <thead>
              <tr>
                <th scope="col">Role</th>
                <th scope="col">Provider and model</th>
                <th scope="col"><span className="sr-only">Action</span></th>
              </tr>
            </thead>
            <tbody>
              {roles.map((r) => (
                <tr key={r}>
                  <td><strong>{r}</strong></td>
                  <td>
                    {assigned[r] ? (
                      <>
                        {`${list.find((p) => p.id === assigned[r].provider_id)?.name ?? "Unknown provider"} / ${assigned[r].model}`} <Badge status="saved" tone="ok" />
                      </>
                    ) : (
                      <Badge status="not set" tone="warn" />
                    )}
                  </td>
                  <td style={{ textAlign: "right" }}>
                    <button className="secondary small" disabled={!rp || !rm} onClick={() => assign([r])}>
                      Use Selection
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </div>
    </>
  );
}
