"use client";
import Link from "next/link";
import { useEffect, useState } from "react";
import { api } from "../../lib/api";

export default function Settings() {
  const [s, setS] = useState<any>(null);
  const [msg, setMsg] = useState("");
  useEffect(() => {
    api("/settings").then(setS).catch((e) => setMsg(e.message));
  }, []);
  if (!s)
    return (
      <p>
        {msg || "Loading..."} {msg && <Link href="/">Sign in</Link>}
      </p>
    );

  async function save(patch: object) {
    try {
      setS(await api("/settings", "PUT", patch));
      setMsg("Saved");
    } catch (e: any) {
      setMsg(e.message);
    }
  }
  return (
    <>
      <h1>Settings</h1>
      <label htmlFor="ms">Merge style</label>
      <select id="ms" value={s.merge_style} onChange={(e) => save({ merge_style: e.target.value })}>
        <option>merge</option>
        <option>squash</option>
        <option>rebase</option>
      </select>
      <label htmlFor="ib">Integration branch</label>
      <input id="ib" defaultValue={s.integration_branch} onBlur={(e) => save({ integration_branch: e.target.value })} />
      <label htmlFor="rr">Review round limit</label>
      <input
        id="rr"
        type="number"
        min={1}
        defaultValue={s.review_round_limit}
        onBlur={(e) => save({ review_round_limit: Number(e.target.value) })}
      />
      <div className="row">
        <label>
          <input
            type="checkbox"
            checked={s.decide_with_defaults}
            onChange={(e) => save({ decide_with_defaults: e.target.checked })}
          />{" "}
          Decide with defaults
        </label>
      </div>
      <p className="muted">
        Feature flag external providers: {String(s.feature_flags.external_providers)} (hidden for now).
      </p>
      {msg && <p role="status">{msg}</p>}
    </>
  );
}
