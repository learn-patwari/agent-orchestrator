import type { ReactNode } from "react";

type Tone = "ok" | "warn" | "err" | "info" | "";

const STATUS_TONE: Record<string, Tone> = {
  approved: "ok", merged: "ok", tested: "ok", ready: "ok", done: "ok", idle: "", active: "info", running: "info",
  developing: "info", dev_done: "info", draft: "", pending: "warn", open: "warn", held: "warn", changes_requested: "warn",
  escalated: "err", failed: "err", blocked: "err",
};

export function Badge({ status, tone }: { status: string; tone?: Tone }) {
  const t = tone ?? STATUS_TONE[status] ?? "";
  return <span className={`badge ${t}`}>{status.replace(/_/g, " ")}</span>;
}

export function PageHead({ title, sub, children }: { title: string; sub?: string; children?: ReactNode }) {
  return (
    <header className="page-head">
      <h1>{title}</h1>
      {sub && <p>{sub}</p>}
      {children}
    </header>
  );
}

export function Empty({ title, children }: { title: string; children?: ReactNode }) {
  return (
    <div className="empty">
      <strong>{title}</strong>
      {children}
    </div>
  );
}

export function Alert({ kind = "error", children }: { kind?: "error" | "warn" | "ok" | "info"; children: ReactNode }) {
  return (
    <p className={`alert ${kind}`} role={kind === "error" ? "alert" : "status"}>
      {children}
    </p>
  );
}
