"use client";
import { useEffect, useState } from "react";
import { api } from "../lib/api";
import { Alert } from "./ui";

type Info = { repo_path: string; exists: boolean; is_git: boolean; worktree_root: string; worktree_root_custom: boolean };

/** Where agents write code: the folder of the project being built, and (optionally) where task worktrees are kept. */
export default function DevFolderCard() {
  const [info, setInfo] = useState<Info | null>(null);
  const [repo, setRepo] = useState("");
  const [wt, setWt] = useState("");
  const [msg, setMsg] = useState("");
  const [err, setErr] = useState("");

  async function load() {
    try {
      const i: Info = await api("/dev-folder");
      setInfo(i);
      setRepo(i.repo_path);
      setWt(i.worktree_root_custom ? i.worktree_root : "");
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
    setMsg("");
    try {
      await api("/settings", "PUT", { repo_path: repo, worktree_root: wt });
      await load();
      setMsg(repo.trim() ? "Saved." : "Cleared. Agents cannot write code until a development folder is set.");
    } catch (e: any) {
      setErr(e.message);
    }
  }

  const missing = info !== null && !info.repo_path;
  return (
    <form onSubmit={save} className="card" aria-labelledby="dev-h">
      <h2 id="dev-h">Development folder</h2>
      <p className="muted">
        The project agents build. Each task gets its own git worktree of this folder (branch <code>task/&lt;name&gt;</code>), so your own checkout is never edited. The knowledge graph is built from it too.
      </p>
      {missing && <Alert kind="warn">No development folder is set. Agents cannot start tasks, and the knowledge graph cannot be built, until you enter one.</Alert>}
      {err && <Alert>{err}</Alert>}
      <label htmlFor="repo">Path to the project</label>
      <input id="repo" name="repo_path" autoComplete="off" spellCheck={false} placeholder="C:\work\my-project" value={repo} onChange={(e) => setRepo(e.target.value)} />
      <p className="field-note">The folder that holds the project&apos;s <code>.git</code>. Paste it as it is; quotes and spaces around it are removed.</p>
      <label htmlFor="wt">Where task worktrees go (optional)</label>
      <input id="wt" name="worktree_root" autoComplete="off" spellCheck={false} placeholder={info ? info.worktree_root : ""} value={wt} onChange={(e) => setWt(e.target.value)} />
      <p className="field-note">Leave empty to use the app&apos;s own folder (the grey text). It must be outside the project folder.</p>
      {info?.repo_path ? (
        <p role="status">
          {info.exists ? (info.is_git ? "Folder found and it is a git repository." : "Folder found, but it is not a git repository yet. Run `git init` in it (and make a first commit) before agents start tasks.") : "That folder does not exist."}
        </p>
      ) : null}
      <div className="row">
        <button type="submit">Save development folder</button>
      </div>
      {msg && <Alert kind="ok">{msg}</Alert>}
    </form>
  );
}
