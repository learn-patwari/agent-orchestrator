"use client";
import { useEffect, useState } from "react";
import { Alert, Badge, Empty, PageHead } from "../../components/ui";
import { api } from "../../lib/api";

export default function Inbox() {
  const [items, setItems] = useState<any[]>([]);
  const [assumptions, setAssumptions] = useState<any[]>([]);
  const [notes, setNotes] = useState<any[]>([]);
  const [answers, setAnswers] = useState<Record<string, string>>({});
  const [err, setErr] = useState("");

  async function load() {
    try {
      setItems(await api("/inbox?status=open"));
      setAssumptions(await api("/assumptions"));
      setNotes(await api("/notifications?unread=true"));
    } catch (e: any) {
      setErr(e.message);
    }
  }
  useEffect(() => {
    load();
  }, []);

  async function act(path: string, body?: unknown) {
    setErr("");
    try {
      await api(path, "POST", body);
      await load();
    } catch (e: any) {
      setErr(e.message);
    }
  }

  return (
    <>
      <PageHead title="Inbox" sub="Everything that needs a decision from you, in one place." />
      {err && <Alert>{err}</Alert>}
      <h2>Needs you ({items.length})</h2>
      {items.length === 0 && <Empty title="All clear">Nothing is waiting for you.</Empty>}
      {items.map((i) => (
        <div className="card" key={i.id}>
          <strong>{i.title}</strong> <Badge status={i.kind} tone={i.kind === "approval" ? "warn" : "info"} />
          <p>{i.body}</p>
          {i.kind === "approval" ? (
            <div className="row">
              <button onClick={() => act(`/inbox/${i.id}/decide`, { approve: true })}>Approve</button>
              <button className="secondary" onClick={() => act(`/inbox/${i.id}/decide`, { approve: false })}>
                Reject
              </button>
            </div>
          ) : (
            <div className="row">
              {i.default && <button onClick={() => act(`/inbox/${i.id}/accept-default`)}>Accept default: {i.default}</button>}
              {i.options?.map((o: string) => (
                <button key={o} className="secondary" onClick={() => act(`/inbox/${i.id}/answer`, { answer: o })}>
                  {o}
                </button>
              ))}
              <input
                aria-label={`Answer for ${i.title}`}
                name="answer" autoComplete="off" placeholder="Or type your own answer…" style={{ flex: "1 1 220px" }}
                value={answers[i.id] ?? ""}
                onChange={(e) => setAnswers({ ...answers, [i.id]: e.target.value })}
                onKeyDown={(e) => {
                  if (e.key === "Enter" && answers[i.id]?.trim()) act(`/inbox/${i.id}/answer`, { answer: answers[i.id] });
                }}
              />
              <button className="secondary" disabled={!answers[i.id]} onClick={() => act(`/inbox/${i.id}/answer`, { answer: answers[i.id] })}>
                Answer
              </button>
            </div>
          )}
        </div>
      ))}
      <h2>Assumptions to review ({assumptions.filter((a) => a.status === "assumed").length})</h2>
      {assumptions.length === 0 && <Empty title="No assumptions">Defaults the agents assumed on your behalf will be listed here.</Empty>}
      {assumptions.map((a) => (
        <div className="card" key={a.id}>
          <strong>[A]</strong> {a.question}: <em>{a.assumed}</em> <span className="muted">({a.status}, {a.source})</span>
          {a.status === "overruled" && <span> changed to {a.overruled_to}</span>}
          {a.status === "assumed" && (
            <div className="row">
              <input
                aria-label={`Overrule ${a.question}`}
                value={answers[a.id] ?? ""}
                onChange={(e) => setAnswers({ ...answers, [a.id]: e.target.value })}
                onKeyDown={(e) => {
                  if (e.key === "Enter" && answers[a.id]?.trim()) act(`/assumptions/${a.id}/overrule`, { value: answers[a.id] });
                }}
                name="overrule" autoComplete="off" placeholder="New value…" style={{ flex: "1 1 220px" }}
              />
              <button className="secondary" disabled={!answers[a.id]} onClick={() => act(`/assumptions/${a.id}/overrule`, { value: answers[a.id] })}>
                Overrule
              </button>
            </div>
          )}
        </div>
      ))}
      <h2>Notifications ({notes.length})</h2>
      {notes.length === 0 && <Empty title="No unread notifications" />}
      {notes.map((n) => (
        <div className="row" key={n.id}>
          <span>
            <strong>{n.title}</strong> <Badge status={n.kind} />
          </span>
          <button className="secondary" onClick={() => act(`/notifications/${n.id}/read`)}>
            Mark read
          </button>
        </div>
      ))}
    </>
  );
}
