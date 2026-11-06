"use client";
import { useEffect, useState } from "react";
import { api } from "../../lib/api";

export default function Status() {
  const [rows, setRows] = useState<any[]>([]);
  const [err, setErr] = useState("");
  useEffect(() => {
    const load = () => api("/status").then(setRows).catch((e) => setErr(e.message));
    load();
    const t = setInterval(load, 5000);
    return () => clearInterval(t);
  }, []);
  return (
    <>
      <h1>Agents</h1>
      {err && (
        <p className="err" role="alert">
          {err}
        </p>
      )}
      <table style={{ width: "100%", borderCollapse: "collapse" }}>
        <thead>
          <tr>
            <th align="left">Role</th>
            <th align="left">State</th>
            <th align="left">Task</th>
            <th align="left">Steps</th>
          </tr>
        </thead>
        <tbody>
          {rows.map((r) => (
            <tr key={r.role}>
              <td>{r.role}</td>
              <td>{r.state}</td>
              <td>{r.task ?? "-"}</td>
              <td>{r.steps}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </>
  );
}
