"use client";
import { useEffect, useState } from "react";
import { Alert, Badge, Empty, PageHead } from "../../components/ui";
import { api } from "../../lib/api";

const BLANK = { name: "", transport: "stdio", command: "", url: "", roles: "", env: "" };

export default function Mcp() {
  const [list, setList] = useState<any[]>([]);
  const [loaded, setLoaded] = useState(false); // no empty message until the first answer arrives
  const [f, setF] = useState(BLANK);
  const [err, setErr] = useState("");
  const [tests, setTests] = useState<Record<string, any>>({});
  const [testing, setTesting] = useState("");

  async function load() {
    try {
      setList(await api("/mcp/servers"));
      setLoaded(true);
    } catch (e: any) {
      setErr(e.message);
    }
  }
  useEffect(() => {
    load();
  }, []);

  async function save(e: React.FormEvent) {
    e.preventDefault();
    setErr("");
    const env_refs = Object.fromEntries(
      f.env
        .split("\n")
        .map((l) => l.trim())
        .filter(Boolean)
        .map((l) => l.split("=").map((x) => x.trim()) as [string, string])
    );
    const body: any = { transport: f.transport, roles: f.roles.split(",").map((r) => r.trim()).filter(Boolean), env_refs };
    if (f.transport === "stdio") body.command = f.command.trim().split(/\s+/).filter(Boolean);
    else body.url = f.url.trim();
    try {
      await api(`/mcp/servers/${encodeURIComponent(f.name.trim())}`, "PUT", body);
      setF(BLANK);
      await load();
    } catch (e: any) {
      setErr(e.message);
    }
  }

  async function test(name: string) {
    setTesting(name);
    setErr("");
    try {
      setTests({ ...tests, [name]: await api(`/mcp/servers/${encodeURIComponent(name)}/test`, "POST") });
    } catch (e: any) {
      setErr(e.message);
    } finally {
      setTesting("");
    }
  }

  async function remove(name: string) {
    setErr("");
    try {
      await api(`/mcp/servers/${encodeURIComponent(name)}`, "DELETE");
      await load();
    } catch (e: any) {
      setErr(e.message);
    }
  }

  return (
    <>
      <PageHead title="MCP servers" sub="Connect tool servers. A role sees a server's tools only if you list that role. The app works with none." />
      {err && <Alert>{err}</Alert>}
      <form onSubmit={save} className="card">
        <h2>Add or change a server</h2>
        <label htmlFor="mn">Name</label>
        <input id="mn" name="name" autoComplete="off" placeholder="jira" value={f.name} onChange={(e) => setF({ ...f, name: e.target.value })} required />
        <label htmlFor="mt">Transport</label>
        <select id="mt" name="transport" value={f.transport} onChange={(e) => setF({ ...f, transport: e.target.value })}>
          <option value="stdio">stdio (a program on this machine)</option>
          <option value="http">http (a server URL)</option>
        </select>
        {f.transport === "stdio" ? (
          <>
            <label htmlFor="mc">Command</label>
            <input id="mc" name="command" autoComplete="off" placeholder="npx -y some-mcp-server" value={f.command} onChange={(e) => setF({ ...f, command: e.target.value })} required />
          </>
        ) : (
          <>
            <label htmlFor="mu">URL</label>
            <input id="mu" name="url" type="url" autoComplete="off" placeholder="https://mcp.example.com/rpc" value={f.url} onChange={(e) => setF({ ...f, url: e.target.value })} required />
          </>
        )}
        <label htmlFor="mr">Roles that may use it (comma separated)</label>
        <input id="mr" name="roles" autoComplete="off" placeholder="developer_1, developer_2" value={f.roles} onChange={(e) => setF({ ...f, roles: e.target.value })} />
        <label htmlFor="me">Secrets by reference (one per line, SERVER_VAR=NAME_OF_ENV_VAR)</label>
        <textarea id="me" name="env" rows={2} autoComplete="off" placeholder="API_TOKEN=JIRA_TOKEN" value={f.env} onChange={(e) => setF({ ...f, env: e.target.value })} />
        <p className="muted">The secret itself is never stored here. It is read from that environment variable on this machine when the server starts.</p>
        <div className="row">
          <button type="submit">Save server</button>
        </div>
      </form>
      {loaded && list.length === 0 ? (
        <Empty title="No MCP servers">Add one above when you want agents to use outside tools.</Empty>
      ) : (
        <ul className="plain" aria-label="MCP servers">
          {list.map((s) => {
            const t = tests[s.name];
            return (
              <li key={s.name} className="card">
                <h2>
                  {s.name} <Badge status={s.enabled ? "enabled" : "disabled"} />
                </h2>
                <p className="muted">
                  {s.transport === "stdio" ? (s.command ?? []).join(" ") : s.url}. Roles: {(s.roles ?? []).join(", ") || "none, so no agent can use it yet"}.
                </p>
                {t && (t.ok ? <Alert kind="ok">Connected. Tools: {t.tools.map((x: any) => x.name).join(", ") || "none"}.</Alert> : <Alert>{t.error}</Alert>)}
                <div className="row">
                  <button type="button" onClick={() => test(s.name)} disabled={testing === s.name}>
                    {testing === s.name ? "Testing…" : "Test connection"}
                  </button>
                  <button type="button" className="secondary" onClick={() => remove(s.name)}>
                    Delete
                  </button>
                </div>
              </li>
            );
          })}
        </ul>
      )}
    </>
  );
}
