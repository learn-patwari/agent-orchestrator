"use client";
import Link from "next/link";
import { useEffect, useState } from "react";
import { Alert, PageHead } from "../../components/ui";
import { api } from "../../lib/api";

export default function Settings() {
  const [s, setS] = useState<any>(null);
  const [msg, setMsg] = useState("");
  const [failed, setFailed] = useState(false);
  useEffect(() => {
    api("/settings").then(setS).catch((e) => { setMsg(e.message); setFailed(true); });
  }, []);
  if (!s)
    return failed ? (
      <Alert>
        {msg} <Link href="/">Sign in</Link> to continue.
      </Alert>
    ) : (
      <p className="muted" role="status">Loading…</p>
    );

  async function save(patch: object) {
    try {
      setS(await api("/settings", "PUT", patch));
      setFailed(false);
      setMsg("Saved");
    } catch (e: any) {
      setFailed(true);
      setMsg(e.message);
    }
  }
  return (
    <>
      <PageHead title="Settings" sub="Changes save as soon as you leave a field." />
      <div className="card">
        <h2>Merging</h2>
        <label htmlFor="ms">Merge style</label>
        <select id="ms" name="merge_style" value={s.merge_style} onChange={(e) => save({ merge_style: e.target.value })}>
          <option>merge</option>
          <option>squash</option>
          <option>rebase</option>
        </select>
        <label htmlFor="ib">Integration branch</label>
        <input id="ib" name="integration_branch" autoComplete="off" spellCheck={false} defaultValue={s.integration_branch} onBlur={(e) => save({ integration_branch: e.target.value })} />
        <p className="field-note">Reviewed work merges here. Merging into main always stays with you.</p>
        <label htmlFor="rr">Review round limit</label>
        <input id="rr" name="review_round_limit" type="number" min={1} inputMode="numeric" autoComplete="off" defaultValue={s.review_round_limit} onBlur={(e) => save({ review_round_limit: Number(e.target.value) })} />
        <p className="field-note">After this many rounds of requested changes, the task comes to you.</p>
      </div>
      <div className="card">
        <h2>Decisions</h2>
        <label style={{ display: "flex", gap: 10, alignItems: "center", fontWeight: 500, margin: 0 }}>
          <input type="checkbox" checked={s.decide_with_defaults} onChange={(e) => save({ decide_with_defaults: e.target.checked })} />
          Decide with recommended defaults
        </label>
        <p className="field-note">Answers open questions with the recommended default and records each one as an assumption you can overrule. Approvals are never automatic.</p>
      </div>
      <p className="muted">External providers: {s.feature_flags.external_providers ? "on" : "off"} (hidden for now).</p>
      {msg && <Alert kind={failed ? "error" : "ok"}>{msg}</Alert>}
    </>
  );
}
