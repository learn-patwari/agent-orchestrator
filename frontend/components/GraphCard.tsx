"use client";
import { useEffect, useState } from "react";
import { api } from "../lib/api";
import { Alert } from "./ui";

type GraphInfo = {
  built: boolean;
  repo: string;
  reason?: string;
  files?: number;
  symbols?: number;
  import_links?: number;
  test_files?: number;
  by_language?: Record<string, number>;
  built_at?: number;
  queries: number;
  tokens_saved: number;
};

/** Builds and shows the code knowledge graph of the repository set in Settings. */
export default function GraphCard() {
  const [g, setG] = useState<GraphInfo | null>(null);
  const [err, setErr] = useState("");
  const [busy, setBusy] = useState(false);

  async function load() {
    try {
      setG(await api("/graph"));
    } catch (e: any) {
      setErr(e.message);
    }
  }
  useEffect(() => {
    load();
  }, []);

  async function build() {
    setBusy(true);
    setErr("");
    try {
      setG(await api("/graph/build", "POST", {}));
    } catch (e: any) {
      setErr(e.message);
    } finally {
      setBusy(false);
    }
  }

  return (
    <section className="card" aria-labelledby="kg-h">
      <h2 id="kg-h">Knowledge graph</h2>
      <p className="muted">
        A map of your repository: files, functions, classes and which file imports which. Agents ask it (find, outline, impact, tests for) instead of reading whole files, which is where most tokens go. It is built by scanning the folder in Settings; no model is used and nothing leaves your computer. Rebuild it after big changes.
      </p>
      {err && <Alert>{err}</Alert>}
      {g && (
        <>
          <p>
            {g.built ? (
              <>
                <strong>{g.files?.toLocaleString()}</strong> files, <strong>{g.symbols?.toLocaleString()}</strong> symbols, {g.import_links?.toLocaleString()} import links, {g.test_files?.toLocaleString()} test files
                {g.by_language ? ` (${Object.entries(g.by_language).map(([k, v]) => `${v} ${k}`).join(", ")})` : ""}. Built {g.built_at ? new Date(g.built_at * 1000).toLocaleString() : ""}.
              </>
            ) : (
              g.reason ?? "Not built yet."
            )}
          </p>
          <p className="muted">
            Answered {g.queries.toLocaleString()} question{g.queries === 1 ? "" : "s"}; about {g.tokens_saved.toLocaleString()} tokens not spent reading whole files (an estimate: an outline instead of the file, 4 bytes to a token).
          </p>
        </>
      )}
      <div className="row">
        <button type="button" onClick={build} disabled={busy || (g !== null && !g.repo)} aria-busy={busy}>
          {busy ? "Scanning…" : g?.built ? "Rebuild graph" : "Build graph"}
        </button>
      </div>
    </section>
  );
}
