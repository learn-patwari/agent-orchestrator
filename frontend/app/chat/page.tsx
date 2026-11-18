"use client";
import { useEffect, useRef, useState } from "react";
import { Alert, PageHead } from "../../components/ui";
import { api } from "../../lib/api";
import { stamp } from "../../lib/stamp";

type Msg = { role: string; content: string; at?: number };
type Waiting = { id: string; kind: string; title: string; body?: string };

export default function Chat() {
  const [msgs, setMsgs] = useState<Msg[]>([]);
  const [text, setText] = useState("");
  const [warn, setWarn] = useState("");
  const [err, setErr] = useState("");
  const [busy, setBusy] = useState(false);
  const [waiting, setWaiting] = useState<Waiting[]>([]);
  const end = useRef<HTMLDivElement>(null);
  const sending = useRef(false);

  // what is waiting for the human, and replies the orchestrator wrote by itself (for example after an approval)
  async function refresh() {
    try {
      const [m, w] = await Promise.all([api<Msg[]>("/chat"), api<Waiting[]>("/inbox?status=open")]);
      if (!sending.current) setMsgs(m);
      setWaiting(w);
    } catch (e: any) {
      setErr(e.message);
    }
  }
  useEffect(() => {
    refresh();
    const t = setInterval(refresh, 5000);
    return () => clearInterval(t);
  }, []);

  async function decide(id: string, approve: boolean) {
    setErr("");
    try {
      await api(`/inbox/${id}/decide`, "POST", { approve });
    } catch (e: any) {
      setErr(e.message);
    }
    refresh();
  }
  useEffect(() => {
    end.current?.scrollIntoView({ block: "end" });
  }, [msgs, busy]);

  async function send(e?: React.FormEvent) {
    e?.preventDefault();
    if (!text.trim() || busy) return;
    setErr("");
    setWarn("");
    setBusy(true);
    sending.current = true;
    const sent = text;
    setMsgs((m) => [...m, { role: "user", content: sent, at: Date.now() / 1000 }]);
    setText("");
    try {
      const scan = await api<{ looks_like_secret: boolean }>("/secrets/scan", "POST", { text: sent });
      if (scan.looks_like_secret) setWarn("This looks like a credential. It will be removed before anything is stored or sent.");
      const r = await api<{ reply: string; warning: string | null }>("/chat", "POST", { text: sent });
      if (r.warning) setWarn(r.warning);
    } catch (e: any) {
      setErr(e.message);
      setText(sent); // keep what you typed so you can retry
    } finally {
      sending.current = false;
      await refresh();
      setBusy(false);
    }
  }

  return (
    <div className="chat">
      <PageHead title="Orchestrator" sub="The only agent that talks to you. Describe a feature or ask for status." />
      <div className="chat-log" role="log" aria-live="polite" aria-label="Conversation">
        {msgs.length === 0 && !busy && (
          <div className="empty">
            <strong>No messages yet</strong>
            Try: “Add a sign-in page with email and password.”
          </div>
        )}
        {msgs.map((m, i) => (
          <div key={i} className={`msg ${m.role === "user" ? "user" : "assistant"}`}>
            <small>{m.role === "user" ? "You" : "Orchestrator"}{m.at ? <time dateTime={new Date(m.at * 1000).toISOString()}> · {stamp(m.at)}</time> : null}</small>
            {m.content}
          </div>
        ))}
        {busy && (
          <div className="msg assistant" role="status">
            <small>Orchestrator</small>
            <span className="muted">Thinking…</span>
          </div>
        )}
        <div ref={end} />
      </div>
      {waiting.length > 0 && (
        <div className="card waiting" role="region" aria-label="Waiting for you">
          <strong>Waiting for you ({waiting.length})</strong>
          {waiting.slice(0, 3).map((w) => (
            <div key={w.id} className="waiting-row">
              <span>{w.title}</span>
              {w.kind === "approval" ? (
                <span className="row">
                  <button type="button" onClick={() => decide(w.id, true)}>Approve</button>
                  <button type="button" className="secondary" onClick={() => decide(w.id, false)}>Reject</button>
                </span>
              ) : (
                <a href="/inbox/">Answer in Inbox</a>
              )}
            </div>
          ))}
          {waiting.length > 3 && <a href="/inbox/">See all in Inbox</a>}
        </div>
      )}
      {warn && <Alert kind="warn">{warn}</Alert>}
      {err && <Alert>{err}</Alert>}
      <form onSubmit={send} className="composer">
        <label htmlFor="msg" className="sr-only">Message</label>
        <textarea
          id="msg" name="message" rows={1} value={text} onChange={(e) => setText(e.target.value)} autoComplete="off"
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
              e.preventDefault();
              send();
            }
          }}
          placeholder="Describe a feature or ask for status…"
        />
        <button type="submit" disabled={busy || !text.trim()}>{busy ? "Sending…" : "Send"}</button>
      </form>
    </div>
  );
}
