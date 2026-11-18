"use client";
import { useEffect, useRef, useState } from "react";
import { api } from "../lib/api";
import { stamp } from "../lib/stamp";

type Msg = { role: string; content: string; at?: number };
type Session = { id: string; title: string; last_at: number; messages: number };

/** A side chat with one agent. Each agent keeps its own saved sessions; the orchestrator keeps the main chat and is the only one that can start other agents. */
export default function AgentChat({ role, label, onClose }: { role: string; label: string; onClose: () => void }) {
  const boss = role === "orchestrator";
  const [sessions, setSessions] = useState<Session[]>([]);
  const [sid, setSid] = useState("");
  const [msgs, setMsgs] = useState<Msg[]>([]);
  const [text, setText] = useState("");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState("");
  const end = useRef<HTMLDivElement>(null);

  const loadSessions = () => {
    if (!boss) api<Session[]>(`/agents/${role}/chat/sessions`).then(setSessions).catch(() => {});
  };
  async function open(id: string) {
    setSid(id);
    setErr("");
    try {
      setMsgs(await api<Msg[]>(boss ? "/chat" : `/agents/${role}/chat?session=${id}`));
    } catch (e: any) {
      setErr(e.detail ?? e.message);
    }
  }
  useEffect(() => {
    setSid("");
    setMsgs([]);
    setErr("");
    loadSessions();
    if (boss) open("main");
  }, [role]);
  useEffect(() => {
    end.current?.scrollIntoView?.({ block: "end" });
  }, [msgs, busy]);

  async function send(e: React.FormEvent) {
    e.preventDefault();
    const t = text.trim();
    if (!t || busy) return;
    setBusy(true);
    setErr("");
    setMsgs((m) => [...m, { role: "user", content: t, at: Date.now() / 1000 }]);
    setText("");
    try {
      const r = await api<{ reply: string; session_id?: string }>(boss ? "/chat" : `/agents/${role}/chat`, "POST", boss ? { text: t } : { text: t, session_id: sid || undefined });
      setMsgs((m) => [...m, { role: "assistant", content: r.reply, at: Date.now() / 1000 }]);
      if (r.session_id) setSid(r.session_id);
      loadSessions();
    } catch (e: any) {
      setErr(e.detail ?? e.message);
    } finally {
      setBusy(false);
    }
  }

  return (
    <aside className="agent-chat" aria-label={`Chat with ${label}`}>
      <div className="row" style={{ justifyContent: "space-between", margin: 0 }}>
        <strong>Chat with {label}</strong>
        <button className="secondary small" onClick={onClose}>Close</button>
      </div>
      {!boss && (
        <div className="row" style={{ margin: "8px 0" }}>
          <label htmlFor="ac-s" className="sr-only">Session</label>
          <select id="ac-s" value={sid} onChange={(e) => (e.target.value ? open(e.target.value) : (setSid(""), setMsgs([])))}>
            <option value="">New session</option>
            {sessions.map((s) => (
              <option key={s.id} value={s.id}>{s.title || "(empty)"} · {s.messages} msgs</option>
            ))}
          </select>
          <button className="secondary small" onClick={() => (setSid(""), setMsgs([]))}>New</button>
        </div>
      )}
      <div className="agent-chat-log" role="log" aria-live="polite">
        {msgs.length === 0 && <p className="muted">{boss ? "Your main chat with the orchestrator." : `Ask ${label} about its work. This chat is only with ${label}; it cannot start other agents.`}</p>}
        {msgs.map((m, i) => (
          <p key={i} className={m.role === "user" ? "ac-me" : "ac-them"}>
            {m.content}
            {m.at ? <time className="ac-time" dateTime={new Date(m.at * 1000).toISOString()}>{stamp(m.at)}</time> : null}
          </p>
        ))}
        {busy && <p className="muted" role="status">{label} is answering…</p>}
        <div ref={end} />
      </div>
      {err && <p className="err" role="alert">{err}</p>}
      <form onSubmit={send} className="row" style={{ margin: 0 }}>
        <label htmlFor="ac-t" className="sr-only">Message to {label}</label>
        <input id="ac-t" autoComplete="off" style={{ flex: "1 1 160px" }} placeholder={`Message ${label}…`} value={text} onChange={(e) => setText(e.target.value)} />
        <button type="submit" disabled={busy || !text.trim()}>Send</button>
      </form>
    </aside>
  );
}
