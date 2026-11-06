"use client";
import { useEffect, useState } from "react";
import { api } from "../../lib/api";

export default function Providers() {
  const [list, setList] = useState<any[]>([]);
  const [types, setTypes] = useState<string[]>([]);
  const [presets, setPresets] = useState<any[]>([]);
  const [pid, setPid] = useState("openai");
  const [roles, setRoles] = useState<string[]>([]);
  const [assigned, setAssigned] = useState<Record<string, { provider_id: string; model: string }>>({});
  const [rp, setRp] = useState("");
  const [rm, setRm] = useState("");
  const [f, setF] = useState({ name: "", type: "openai", base_url: "", api_key: "" });
  const [model, setModel] = useState("");
  const [res, setRes] = useState<Record<string, string>>({});
  const [err, setErr] = useState("");

  async function load() {
    try {
      setList(await api("/providers"));
      setTypes((await api("/providers/types")).types);
      setPresets((await api("/providers/presets")).presets);
      const rmod = await api("/role-models");
      setRoles(rmod.roles);
      setAssigned(rmod.models);
    } catch (e: any) {
      setErr(e.message);
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
      setF({ ...f, name: "", api_key: "" });
      load();
    } catch (e: any) {
      setErr(e.message);
    }
  }
  async function test(id: string) {
    const r = await api(`/providers/${id}/test`, "POST", { model });
    setRes({
      ...res,
      [id]: r.ok ? `OK (${r.usage.input_tokens}/${r.usage.output_tokens} tokens)` : `Failed (${r.error_kind}): ${r.error}`,
    });
  }
  async function assign(rolesToSet: string[]) {
    setErr("");
    try {
      for (const r of rolesToSet) await api(`/role-models/${r}`, "PUT", { provider_id: rp, model: rm });
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
  return (
    <>
      <h1>LLM providers</h1>
      <form onSubmit={add} className="card">
        <label htmlFor="n">Name</label>
        <input id="n" value={f.name} onChange={(e) => setF({ ...f, name: e.target.value })} required />
        <label htmlFor="pr">Provider</label>
        <select id="pr" value={pid} onChange={(e) => choose(e.target.value)}>
          {presets.map((p) => (
            <option key={p.id} value={p.id}>
              {p.name}
            </option>
          ))}
        </select>
        {preset?.note && <p className="muted">{preset.note}</p>}
        {pid === "custom" && (
          <>
            <label htmlFor="t">Protocol</label>
            <select id="t" value={f.type} onChange={(e) => setF({ ...f, type: e.target.value })}>
              {types
                .filter((t) => t !== "claude_cli")
                .map((t) => (
                  <option key={t}>{t}</option>
                ))}
            </select>
          </>
        )}
        <label htmlFor="u">{cli ? "Path to claude.exe (optional)" : "Base URL"}</label>
        <input
          id="u"
          size={40}
          placeholder={cli ? "leave empty to find it automatically" : "https://api.example.com/v1"}
          value={f.base_url}
          onChange={(e) => setF({ ...f, base_url: e.target.value })}
          required={!cli}
        />
        {!cli && (
          <>
            <label htmlFor="k">API key{needsKey ? "" : " (not needed)"}</label>
            <input
              id="k"
              type="password"
              autoComplete="off"
              value={f.api_key}
              onChange={(e) => setF({ ...f, api_key: e.target.value })}
              required={needsKey}
            />
          </>
        )}
        <div className="row">
          <button type="submit">Add provider</button>
        </div>
      </form>
      {err && (
        <p className="err" role="alert">
          {err}
        </p>
      )}
      <label htmlFor="m">Model for test connection</label>
      <input id="m" value={model} onChange={(e) => setModel(e.target.value)} placeholder="model name" />
      <h2>Models for roles</h2>
      <p className="muted">The Orchestrator and every agent need a provider and model before chat works.</p>
      <div className="card">
        <label htmlFor="rp">Provider</label>
        <select id="rp" value={rp} onChange={(e) => setRp(e.target.value)}>
          <option value="">choose...</option>
          {list.map((p) => (
            <option key={p.id} value={p.id}>
              {p.name}
            </option>
          ))}
        </select>
        <label htmlFor="rm">Model</label>
        <input id="rm" value={rm} onChange={(e) => setRm(e.target.value)} placeholder="e.g. sonnet, gpt-4o, gemini-2.0-flash" />
        <div className="row">
          <button disabled={!rp || !rm} onClick={() => assign(roles)}>
            Use for all roles
          </button>
        </div>
        <table style={{ width: "100%", borderCollapse: "collapse", marginTop: 12 }}>
          <tbody>
            {roles.map((r) => (
              <tr key={r}>
                <td>{r}</td>
                <td className="muted">
                  {assigned[r]
                    ? `${list.find((p) => p.id === assigned[r].provider_id)?.name ?? "?"} / ${assigned[r].model}`
                    : "not set"}
                </td>
                <td>
                  <button className="secondary" disabled={!rp || !rm} onClick={() => assign([r])}>
                    Set to selection
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      <h2>Providers</h2>
      {list.map((p) => (
        <div className="card" key={p.id}>
          <strong>{p.name}</strong>{" "}
          <span className="muted">
            {p.type} / {p.base_url || "auto-detect"} / {p.type === "claude_cli" ? "Claude login" : `key ${p.api_key}`}
          </span>
          <div className="row">
            <button className="secondary" disabled={!model} onClick={() => test(p.id)}>
              Test connection
            </button>
            <button
              className="secondary"
              onClick={async () => {
                await api(`/providers/${p.id}`, "DELETE");
                load();
              }}
            >
              Delete
            </button>
            <span role="status">{res[p.id]}</span>
          </div>
        </div>
      ))}
    </>
  );
}
