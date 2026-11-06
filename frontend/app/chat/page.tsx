"use client";
import { useEffect, useRef, useState } from "react";
import { Alert, PageHead } from "../../components/ui";
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
    end.current?.scrollIntoView({ block: "end" });
  }, [msgs, busy]);

  async function send(e?: React.FormEvent) {
    e?.preventDefault();
    if (!text.trim() || busy) return;
    setErr("");
    setWarn("");
    setBusy(true);
    const sent = text;
    setMsgs((m) => [...m, { role: "user", content: sent }]);
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
      try {
        setMsgs(await api<Msg[]>("/chat"));
      } catch {}
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
            <small>{m.role === "user" ? "You" : "Orchestrator"}</small>
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
      {warn && <Alert kind="warn">{warn}</Alert>}
      {err && <Alert>{err}</Alert>}
      <form onSubmit={send} className="composer">
        <label htmlFor="msg" className="sr-only">Message</label>
        <textarea
          id="msg" name="message" rows={1} value={text} onChange={(e) => setText(e.target.value)} autoComplete="off"
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.shiftKey) {
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
