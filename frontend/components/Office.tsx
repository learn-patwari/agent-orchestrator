"use client";
import { useEffect, useRef } from "react";
import { CharStyle, Environments, Handoff, OfficeScene, TeamRow } from "../lib/office-scene";

/** The animated office. All state comes from the app (team rows and hand-offs); nothing here is scripted. */
export default function Office({
  team,
  handoffs,
  environments,
  paused,
  look = "robots",
  onSelect,
}: {
  team: TeamRow[];
  handoffs: Handoff[];
  environments: Environments;
  paused: boolean;
  look?: CharStyle;
  onSelect: (role: string) => void;
}) {
  const ref = useRef<HTMLCanvasElement>(null);
  const scene = useRef<OfficeScene | null>(null);
  const pick = useRef(onSelect);
  pick.current = onSelect;

  useEffect(() => {
    if (!ref.current) return;
    const reduce = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    const s = new OfficeScene(ref.current, reduce);
    s.onSelect = (r) => pick.current(r);
    scene.current = s;
    return () => {
      s.destroy();
      scene.current = null;
    };
  }, []);

  useEffect(() => {
    scene.current?.setTeam(team);
  }, [team]);
  useEffect(() => {
    scene.current?.setHandoffs(handoffs);
  }, [handoffs]);
  useEffect(() => {
    scene.current?.setEnvironments(environments);
  }, [environments]);
  useEffect(() => {
    scene.current?.setStyle(look);
  }, [look]);
  useEffect(() => {
    if (scene.current) scene.current.paused = paused;
  }, [paused]);

  return (
    <canvas
      ref={ref}
      className="office-canvas"
      role="img"
      aria-label="Animated office. Each agent (shown as a robot, person, animal, hero or star, your choice) sits at a desk; the office boy carries work between them when the app reports a hand-off. Use the table view for the same information as text."
    />
  );
}
