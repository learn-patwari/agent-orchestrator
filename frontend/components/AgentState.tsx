import { STATUS_COLORS, toStatus } from "../lib/office-scene";

/** An agent's state, drawn the same way in the office, the chips, the details card and the table: the office's colour and word. */
export default function AgentState({ state }: { state: string }) {
  const s = toStatus(state);
  return (
    <span className="agent-state">
      <i className="dot" style={{ background: STATUS_COLORS[s] }} aria-hidden="true" />
      {s}
    </span>
  );
}
