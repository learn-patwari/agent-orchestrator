"use client";
import Link from "next/link";
import { useEffect, useState } from "react";
import { api } from "../lib/api";

type State = "loading" | "setup" | "login" | "in";

export default function Home() {
  const [state, setState] = useState<State>("loading");
  const [pw, setPw] = useState("");
  const [err, setErr] = useState("");

  async function refresh() {
    const s = await api<{ setup_done: boolean }>("/auth/status");
    try {
      await api("/settings");
      setState("in");
    } catch {
      setState(s.setup_done ? "login" : "setup");
    }
  }
  useEffect(() => {
    refresh().catch((e) => setErr(e.message));
  }, []);

  async function submit(kind: "setup" | "login") {
    setErr("");
    try {
      await api(`/auth/${kind}`, "POST", { password: pw });
      setPw("");
      await refresh();
    } catch (e: any) {
      setErr(e.message);
    }
  }

  if (state === "loading") return <p>{err || "Loading..."}</p>;
  if (state === "in")
    return (
      <>
        <h1>Welcome</h1>
        <p>
          Next: add an LLM provider under <Link href="/providers">Providers</Link>.
        </p>
        <button
          className="secondary"
          onClick={async () => {
            await api("/auth/logout", "POST");
            refresh();
          }}
        >
          Sign out
        </button>
      </>
    );
  return (
    <form
      onSubmit={(e) => {
        e.preventDefault();
        submit(state);
      }}
    >
      <h1>{state === "setup" ? "Set a password (first run)" : "Sign in"}</h1>
      <label htmlFor="pw">Password</label>
      <input id="pw" type="password" value={pw} onChange={(e) => setPw(e.target.value)} minLength={8} required />
      <div className="row">
        <button type="submit">{state === "setup" ? "Create" : "Sign in"}</button>
      </div>
      {err && (
        <p className="err" role="alert">
          {err}
        </p>
      )}
    </form>
  );
}
