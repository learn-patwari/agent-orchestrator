"use client";
import { useEffect, useState } from "react";
import { api } from "./api";

/** The office feed (team rows, hand-offs, environments ...) polled every few seconds. One place for the Agents page and the detached window. */
export function useOffice(every = 5000) {
  const [data, setData] = useState<any>(null);
  const [err, setErr] = useState("");
  useEffect(() => {
    const load = () =>
      api("/office")
        .then((r) => {
          setData(r);
          setErr("");
        })
        .catch((e) => setErr(e.message));
    load();
    const t = setInterval(load, every);
    return () => clearInterval(t);
  }, [every]);
  return { data, err, setData };
}
