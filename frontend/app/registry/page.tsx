"use client";
import { useEffect, useState } from "react";
import { Alert, Badge, Empty, PageHead } from "../../components/ui";
import { api } from "../../lib/api";

export default function Registry() {
  const [list, setList] = useState<any[]>([]);
  const [loaded, setLoaded] = useState(false); // no empty message until the first answer arrives
  const [key, setKey] = useState("");
  const [shown, setShown] = useState(false);
  const [err, setErr] = useState("");
  const [msg, setMsg] = useState("");

  async function load() {
    try {
      setList(await api("/registry"));
      setLoaded(true);
      setKey((await api("/registry/key")).key);
    } catch (e: any) {
      setErr(e.message);
    }
  }
  useEffect(() => {
    load();
    const t = setInterval(load, 15000);
    return () => clearInterval(t);
  }, []);

  async function decide(name: string, what: "approve" | "reject") {
    setErr("");
    try {
      await api(`/registry/${encodeURIComponent(name)}/${what}`, "POST");
      await load();
    } catch (e: any) {
      setErr(e.message);
    }
  }

  async function copy() {
    try {
      await navigator.clipboard.writeText(key);
      setMsg("Key copied.");
    } catch {
      setMsg("Could not copy. Select the key and copy it by hand.");
    }
  }

  return (
    <>
      <PageHead title="External agents" sub="Agents that run elsewhere announce themselves here. A new agent waits for your yes, and only an approved agent with a recent heartbeat is offered to the Orchestrator." />
      {err && <Alert>{err}</Alert>}
      <div className="card">
        <h2>Registration key</h2>
        <p className="muted">Give this key to an agent so it can register. It sends it in the X-Registry-Key header to POST /api/registry/register, then keeps sending heartbeats with its own token.</p>
        <div className="row">
          <code aria-label="Registration key">{shown ? key : "••••••••••••••••"}</code>
          <button type="button" className="secondary" onClick={() => setShown(!shown)}>
            {shown ? "Hide" : "Show"}
          </button>
          <button type="button" className="secondary" onClick={copy}>
            Copy
          </button>
        </div>
        {msg && <Alert kind="info">{msg}</Alert>}
        <p className="muted">Sending work to external agents also needs the External providers switch on in Settings.</p>
      </div>
      {loaded && list.length === 0 ? (
        <Empty title="No external agents yet">When an agent registers it will appear here for your approval.</Empty>
      ) : (
        <ul className="plain" aria-label="External agents">
          {list.map((a) => (
            <li key={a.name} className="card">
              <h2>
                {a.name} <Badge status={a.status === "approved" ? (a.live ? "live" : "offline") : a.status} />
              </h2>
              <p>{a.description || "No description."}</p>
              <p className="muted">Can: {(a.capabilities ?? []).join(", ") || "not stated"}. Endpoint {a.endpoint}</p>
              <div className="row">
                {a.status !== "approved" && <button type="button" onClick={() => decide(a.name, "approve")}>Approve</button>}
                {a.status !== "rejected" && (
                  <button type="button" className="secondary" onClick={() => decide(a.name, "reject")}>
                    {a.status === "approved" ? "Revoke" : "Reject"}
                  </button>
                )}
              </div>
            </li>
          ))}
        </ul>
      )}
    </>
  );
}
