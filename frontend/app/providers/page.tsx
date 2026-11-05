"use client";
import { useEffect, useState } from "react";
import { api } from "../../lib/api";

export default function Providers() {
  const [list, setList] = useState<any[]>([]);
  const [types, setTypes] = useState<string[]>([]);
  const [f, setF] = useState({ name: "", type: "openai", base_url: "", api_key: "" });
  const [model, setModel] = useState("");
  const [res, setRes] = useState<Record<string, string>>({});
  const [err, setErr] = useState("");

  async function load() {
    try {
      setList(await api("/providers"));
      setTypes((await api("/providers/types")).types);
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
      [id]: r.ok ? `OK (${r.usage.input_tokens}/${r.usage.output_tokens} tokens)` : `Failed: ${r.error_kind}`,
    });
  }
  return (
    <>
      <h1>LLM providers</h1>
      <form onSubmit={add} className="card">
        <label htmlFor="n">Name</label>
        <input id="n" value={f.name} onChange={(e) => setF({ ...f, name: e.target.value })} required />
        <label htmlFor="t">Type</label>
        <select id="t" value={f.type} onChange={(e) => setF({ ...f, type: e.target.value })}>
          {types.map((t) => (
            <option key={t}>{t}</option>
          ))}
        </select>
        <label htmlFor="u">Base URL</label>
        <input
          id="u"
          size={40}
          placeholder="https://api.example.com/v1"
          value={f.base_url}
          onChange={(e) => setF({ ...f, base_url: e.target.value })}
          required
        />
        <label htmlFor="k">API key</label>
        <input
          id="k"
          type="password"
          autoComplete="off"
          value={f.api_key}
          onChange={(e) => setF({ ...f, api_key: e.target.value })}
          required
        />
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
      {list.map((p) => (
        <div className="card" key={p.id}>
          <strong>{p.name}</strong>{" "}
          <span className="muted">
            {p.type} / {p.base_url} / key {p.api_key}
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
