"use client";
import { useEffect, useRef, useState } from "react";
import { api } from "../../lib/api";

type Msg = { role: string; content: string };

export default function Chat() {
  const [msgs, setMsgs] = useState<Msg[]>([]);
  const [text, setText] = useState("");
  const [warn, setWarn] = useState("");
  const [err, setErr] = useState("");
  const [busy, setBusy] = useState(false);
  const end = useRef<HTMLDivElement>(null);

  useEffect(() => {
    api<Msg[]>("/chat").then(setMsgs).catch((e) => setErr(e.message));
  }, []);
  useEffect(() => {
    end.current?.scrollIntoView();
  }, [msgs]);

  async function send(e: React.FormEvent) {
    e.preventDefault();
    if (!text.trim()) return;
    setErr("");
    setWarn("");
    setBusy(true);
    try {
      const scan = await api<{ looks_like_secret: boolean }>("/secrets/scan", "POST", { text });
      if (scan.looks_like_secret) setWarn("This looks like a credential. It will be removed before anything is stored or sent.");
      const r = await api<{ reply: string; warning: string | null }>("/chat", "POST", { text });
      if (r.warning) setWarn(r.warning);
      setMsgs(await api<Msg[]>("/chat"));
      setText("");
    } catch (e: any) {
      setErr(e.message);
    } finally {
      setBusy(false);
    }
  }

  return (
    <>
      <h1>Orchestrator</h1>
      <p className="muted">The orchestrator is the only agent that talks to you.</p>
      <div className="card" aria-live="polite" style={{ minHeight: 200 }}>
        {msgs.length === 0 && <span className="muted">No messages yet. Describe a feature to start.</span>}
        {msgs.map((m, i) => (
          <p key={i}>
            <strong>{m.role === "user" ? "You" : "Orchestrator"}:</strong> {m.content}
          </p>
        ))}
        <div ref={end} />
      </div>
      {warn && (
        <p className="err" role="alert">
          {warn}
        </p>
      )}
      {err && (
        <p className="err" role="alert">
          {err}
        </p>
      )}
      <form onSubmit={send} className="row">
        <label htmlFor="msg" style={{ display: "none" }}>
          Message
        </label>
        <input id="msg" size={60} value={text} onChange={(e) => setText(e.target.value)} placeholder="Describe a feature or ask for status" />
        <button type="submit" disabled={busy}>
          Send
        </button>
      </form>
    </>
  );
}
