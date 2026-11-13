"use client";
import { useEffect, useState } from "react";
import AgentState from "../../components/AgentState";
import Office from "../../components/Office";
import { CHAR_STYLES, CharStyle, Environments, seatLabel } from "../../lib/office-scene";
import { useOffice } from "../../lib/use-office";

const NO_ENV: Environments = { qc: null, prod: null };

/** The Agents view on its own, for a second monitor. Opened from the Agents page; the sidebar and bars are hidden by CSS. */
export default function AgentsWindow() {
  const { data, err } = useOffice();
  const [look, setLook] = useState<CharStyle>("robots");
  const [selected, setSelected] = useState<string | null>(null);

  useEffect(() => {
    document.title = "Agents";
    try {
      const v = localStorage.getItem("ao.office.look") as CharStyle | null;
      if (v && CHAR_STYLES.some((c) => c.id === v)) setLook(v);
    } catch {}
  }, []);

  const rows: any[] = data?.team ?? [];
  const sel = rows.find((r) => r.role === selected);
  return (
    <div className="detached">
      {err && <p role="alert" className="muted">{err}</p>}
      {rows.length > 0 && (
        <>
          <Office team={rows} handoffs={data.handoffs ?? []} environments={data.environments ?? NO_ENV} paused={false} look={look} onSelect={setSelected} />
          <div className="agent-chips" aria-label="Agent status">
            {rows.map((r) => (
              <button key={r.role} className="agent-chip" onClick={() => setSelected(r.role)} aria-pressed={selected === r.role}>
                <AgentState state={r.state} /> {seatLabel(r.role)}
              </button>
            ))}
          </div>
          {sel && (
            <p className="muted">
              <strong>{seatLabel(sel.role)}</strong>: {sel.task ?? "no task"} · {sel.model ?? "no model"} · {sel.steps} steps
            </p>
          )}
        </>
      )}
    </div>
  );
}
