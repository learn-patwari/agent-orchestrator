// Pixel office scene for the Agents page. Drawing and walking only: it never invents work.
// People sit at their desks; the office boy walks only when the app reports a real hand-off.
// Look and layout follow the design in docs/design/office-view-mockup.html.

export type TeamRow = { role: string; description?: string; model?: string | null; state: string; task: string | null; steps: number };
export type Handoff = { id: string; from: string; to: string; label: string; at: number };

type Pt = [number, number];
type Status = "idle" | "running" | "done" | "failed";
type Mover = {
  x: number; y: number; path: Pt[]; moving: boolean; res: (() => void) | null; speed: number; say: string; sayT: number;
};
type Person = Mover & {
  id: string; label: string; desk: Pt; home: Pt; front: Pt; shirt: string; hair: string; skin: string;
  state: Status; task: string | null; dummy: boolean; carry: string | null; cap?: boolean;
};

const W = 320, H = 200, S = 3;
export const STATUS_COLORS: Record<Status, string> = { running: "#2f9e44", done: "#3b5bdb", failed: "#e03131", idle: "#9aa3b2" };

const ROOMS: Record<string, { x: number; y: number; w: number; h: number; c: string; name: string }> = {
  manager: { x: 0, y: 0, w: 98, h: 92, c: "#d8c9ec", name: "Manager" },
  eng: { x: 104, y: 0, w: 216, h: 72, c: "#c4dbee", name: "Engineering" },
  qc: { x: 104, y: 76, w: 100, h: 44, c: "#e9e0b8", name: "QC" },
  servers: { x: 212, y: 76, w: 108, h: 44, c: "#cfd4dc", name: "Servers" },
  support: { x: 0, y: 98, w: 98, h: 102, c: "#efd3cd", name: "Support L1-L4" },
  lounge: { x: 104, y: 124, w: 216, h: 76, c: "#d4e8cc", name: "Lounge" },
};
const GAPS: number[][] = [[96, 76, 4, 10], [96, 102, 4, 12], [146, 70, 8, 4], [146, 76, 8, 4], [256, 70, 8, 4], [256, 76, 8, 4], [146, 120, 8, 4], [146, 124, 8, 4]];
const NODES: Record<string, Pt> = { mgrOut: [101, 80], supOut: [101, 108], V74: [101, 74], V122: [101, 122], H150: [150, 74], H260: [260, 74], H150b: [150, 122] };
const EDGES: [string, string][] = [["mgrOut", "V74"], ["mgrOut", "supOut"], ["supOut", "V122"], ["V74", "H150"], ["H150", "H260"], ["V122", "H150b"]];
const CFG: Record<string, { lane: number; via: Pt; in: Pt; out: string }> = {
  manager: { lane: 76, via: [96, 76], in: [96, 80], out: "mgrOut" },
  eng: { lane: 62, via: [150, 62], in: [150, 66], out: "H150" },
  qc: { lane: 114, via: [150, 114], in: [150, 82], out: "H150" },
  servers: { lane: 112, via: [260, 112], in: [260, 82], out: "H260" },
  support: { lane: 164, via: [96, 164], in: [96, 108], out: "supOut" },
  lounge: { lane: 130, via: [150, 130], in: [150, 126], out: "H150b" },
};

// role id from the backend -> desk and look
const SEATS: { id: string; label: string; desk: Pt; shirt: string; hair: string; dummy?: boolean }[] = [
  { id: "orchestrator", label: "orchestrator", desk: [36, 34], shirt: "#364fc7", hair: "#222" },
  { id: "architect", label: "architect", desk: [112, 28], shirt: "#f59f00", hair: "#6b3f1d" },
  { id: "developer_1", label: "developer 1", desk: [160, 28], shirt: "#2f9e44", hair: "#3b2412" },
  { id: "developer_2", label: "developer 2", desk: [208, 28], shirt: "#12b886", hair: "#1b1b1b" },
  { id: "cicd", label: "ci/cd", desk: [256, 28], shirt: "#1098ad", hair: "#7a4b21" },
  { id: "tester", label: "tester", desk: [168, 86], shirt: "#ae3ec9", hair: "#d9a441" },
  { id: "incident_reporter", label: "incident reporter", desk: [14, 132], shirt: "#e8590c", hair: "#222" },
  { id: "production_support", label: "production support", desk: [58, 132], shirt: "#5c7cfa", hair: "#a0522d" },
  { id: "receptionist", label: "receptionist", desk: [262, 146], shirt: "#e64980", hair: "#5c3317", dummy: true },
];
const SKIN = ["#f1c7a0", "#e0a97c", "#c68642", "#8d5524"];
const CAT_SPOTS: Pt[] = [[70, 76], [200, 62], [120, 114], [30, 164], [160, 150], [304, 112], [130, 130], [240, 62], [190, 150], [60, 76]];

export function toStatus(s: string): Status {
  if (s === "running") return "running";
  if (s === "done" || s === "completed") return "done";
  if (s === "failed" || s === "error") return "failed";
  return "idle";
}
export function seatLabel(role: string): string {
  return SEATS.find((s) => s.id === role)?.label ?? role.replace(/_/g, " ");
}

function roomAt(x: number, y: number): string | null {
  for (const k in ROOMS) { const r = ROOMS[k]; if (x >= r.x - 1 && x <= r.x + r.w + 1 && y >= r.y - 1 && y <= r.y + r.h + 1) return k; }
  return null;
}
function nodePath(a: string, b: string): Pt[] {
  const dist: Record<string, number> = { [a]: 0 }, prev: Record<string, string> = {}, todo = new Set(Object.keys(NODES));
  while (todo.size) {
    let u: string | null = null;
    for (const n of todo) if (dist[n] !== undefined && (u === null || dist[n] < dist[u])) u = n;
    if (u === null || u === b) break;
    todo.delete(u);
    for (const [p, q] of EDGES) {
      const v = p === u ? q : q === u ? p : null; if (!v) continue;
      const d = dist[u] + Math.hypot(NODES[u][0] - NODES[v][0], NODES[u][1] - NODES[v][1]);
      if (dist[v] === undefined || d < dist[v]) { dist[v] = d; prev[v] = u; }
    }
  }
  const out: Pt[] = [];
  for (let n: string | undefined = b; n !== undefined; n = prev[n]) { out.unshift(NODES[n]); if (n === a) break; }
  return out;
}
/** Walking route between two points, through the doors and corridors. Pure. */
export function route(from: Pt, to: Pt): Pt[] {
  const ra = roomAt(from[0], from[1]), rb = roomAt(to[0], to[1]);
  if (!ra || !rb) return [to];
  if (ra === rb) return [[from[0], CFG[ra].lane], [to[0], CFG[ra].lane], to];
  const A = CFG[ra], B = CFG[rb];
  return [[from[0], A.lane], A.via, A.in, ...nodePath(A.out, B.out), B.in, B.via, [to[0], B.lane], to];
}

export class OfficeScene {
  private ctx: CanvasRenderingContext2D;
  private people: Person[] = [];
  private byId: Record<string, Person> = {};
  private boy: Person;
  private cat: Mover & { sleep: boolean };
  private timers: { t: number; r: () => void }[] = [];
  private flights: { fx: number; fy: number; tx: number; ty: number; t: number; dur: number }[] = [];
  private queue: Handoff[] = [];
  private seen = new Set<string>();
  private primed = false;
  private working = false;
  private raf = 0;
  private last = 0;
  private catTimer = 2;
  paused = false;
  selected: string | null = null;
  onSelect: (role: string) => void = () => {};

  constructor(private cv: HTMLCanvasElement, reduceMotion = false) {
    cv.width = W * S; cv.height = H * S;
    this.ctx = cv.getContext("2d") as CanvasRenderingContext2D;
    this.paused = reduceMotion;
    SEATS.forEach((s, i) => {
      const home: Pt = [s.desk[0] + 12, s.desk[1] + 8];
      const p: Person = {
        id: s.id, label: s.label, desk: s.desk, home, front: [s.desk[0] + 12, s.desk[1] + 24], shirt: s.shirt, hair: s.hair,
        skin: SKIN[i % 4], state: "idle", task: s.dummy ? "waiting" : null, dummy: !!s.dummy, carry: null,
        x: home[0], y: home[1], path: [], moving: false, res: null, speed: 0, say: "", sayT: 0,
      };
      this.people.push(p); this.byId[s.id] = p;
    });
    this.boy = { id: "office boy", label: "office boy", desk: [0, 0], home: [214, 160], front: [214, 160], shirt: "#868e96", hair: "#222",
      skin: SKIN[1], state: "idle", task: null, dummy: true, carry: null, cap: true,
      x: 214, y: 160, path: [], moving: false, res: null, speed: 95, say: "", sayT: 0 };
    this.cat = { x: 160, y: 150, path: [], moving: false, res: null, speed: 34, say: "", sayT: 0, sleep: false };
    cv.addEventListener("click", this.click);
    this.raf = requestAnimationFrame(this.frame);
  }

  destroy() { cancelAnimationFrame(this.raf); this.cv.removeEventListener("click", this.click); }

  /** Real state from the app. Desks only change colour, bubble and screen; nobody moves because of it. */
  setTeam(rows: TeamRow[]) {
    for (const r of rows) {
      const p = this.byId[r.role]; if (!p || p.dummy) continue;
      p.state = toStatus(r.state); p.task = r.task;
    }
  }

  /** Real hand-offs from the app. The first batch is history (not replayed); later ones send the office boy. */
  setHandoffs(list: Handoff[]) {
    if (!this.primed) { list.forEach((h) => this.seen.add(h.id)); this.primed = true; return; }
    for (const h of list) if (!this.seen.has(h.id)) { this.seen.add(h.id); this.queue.push(h); }
    void this.work();
  }

  private wait(ms: number) { return new Promise<void>((r) => this.timers.push({ t: ms / 1000, r })); }
  private walk(p: Mover, pts: Pt[]) {
    if (p.res) { const r = p.res; p.res = null; r(); }
    p.path = pts.map((a) => [a[0], a[1]] as Pt); p.moving = pts.length > 0;
    return new Promise<void>((r) => { p.res = r; if (!pts.length) { p.res = null; r(); } });
  }
  private goTo(p: Mover, dest: Pt) { return this.walk(p, route([p.x, p.y], dest)); }

  private async work() {
    if (this.working) return;
    this.working = true;
    while (this.queue.length) {
      const h = this.queue.shift() as Handoff;
      const a = this.byId[h.from], b = this.byId[h.to]; if (!a || !b) continue;
      await this.goTo(this.boy, a.front);
      a.say = h.label.slice(0, 24); a.sayT = 1.4; await this.wait(250); this.boy.carry = h.label;
      await this.goTo(this.boy, b.front);
      this.flights.push({ fx: this.boy.x + 4, fy: this.boy.y - 12, tx: b.x, ty: b.y - 8, t: 0, dur: 0.35 });
      this.boy.carry = null; await this.wait(380); b.say = "Got it"; b.sayT = 1.6; await this.wait(200);
    }
    this.working = false;
    void this.goTo(this.boy, this.boy.home);
  }

  private click = (e: MouseEvent) => {
    const r = this.cv.getBoundingClientRect(), lx = ((e.clientX - r.left) / r.width) * W, ly = ((e.clientY - r.top) / r.height) * H;
    const hit = this.people.filter((p) => Math.abs(lx - p.x) < 8 && ly > p.y - 18 && ly < p.y + 3).sort((a, b) => b.y - a.y)[0];
    if (hit) { this.selected = hit.id; this.onSelect(hit.id); }
  };

  private update(dt: number) {
    for (let i = this.timers.length - 1; i >= 0; i--) { this.timers[i].t -= dt; if (this.timers[i].t <= 0) { const r = this.timers[i].r; this.timers.splice(i, 1); r(); } }
    this.catTimer -= dt;
    if (this.catTimer <= 0 && !this.cat.moving) {
      this.catTimer = 3 + Math.random() * 5;
      this.cat.sleep = false;
      void this.goTo(this.cat, CAT_SPOTS[Math.floor(Math.random() * CAT_SPOTS.length)]).then(() => { this.cat.sleep = Math.random() < 0.5; });
    }
    for (const p of [...this.people, this.boy, this.cat] as Mover[]) {
      if (p.sayT > 0) { p.sayT -= dt; if (p.sayT <= 0) p.say = ""; }
      if (p.path.length) {
        const [tx, ty] = p.path[0], dx = tx - p.x, dy = ty - p.y, d = Math.hypot(dx, dy), step = p.speed * dt;
        if (d <= step) { p.x = tx; p.y = ty; p.path.shift(); if (!p.path.length) { p.moving = false; const r = p.res; p.res = null; if (r) r(); } }
        else { p.x += (dx / d) * step; p.y += (dy / d) * step; }
      }
    }
    for (let i = this.flights.length - 1; i >= 0; i--) { this.flights[i].t += dt; if (this.flights[i].t >= this.flights[i].dur) this.flights.splice(i, 1); }
  }

  private frame = (ts: number) => {
    const dt = Math.min(0.05, (ts - this.last) / 1000); this.last = ts;
    if (!this.paused) this.update(dt);
    this.draw(ts / 1000);
    this.raf = requestAnimationFrame(this.frame);
  };

  // ---------- drawing ----------
  private px(x: number, y: number, w: number, h: number, c: string) { this.ctx.fillStyle = c; this.ctx.fillRect(Math.round(x), Math.round(y), w, h); }
  private paper(x: number, y: number) { this.px(x, y, 5, 6, "#fff"); this.px(x, y, 5, 1, "#ced4da"); this.px(x + 1, y + 2, 3, 1, "#adb5bd"); this.px(x + 1, y + 4, 3, 1, "#adb5bd"); }
  private plant(x: number, y: number) { this.px(x + 1, y + 6, 4, 3, "#9c5a2e"); this.px(x, y + 2, 6, 4, "#2f9e44"); this.px(x + 1, y, 4, 3, "#51cf66"); }

  private drawRooms() {
    const c = this.ctx, px = this.px.bind(this);
    px(0, 0, W, H, "#cfc6b4");
    for (const k in ROOMS) { const r = ROOMS[k]; px(r.x, r.y, r.w, r.h, "#5b5368"); px(r.x + 2, r.y + 2, r.w - 4, r.h - 4, r.c); }
    GAPS.forEach((g) => px(g[0], g[1], g[2], g[3], "#cfc6b4"));
    c.font = "bold 4.6px ui-monospace,Consolas,monospace"; c.fillStyle = "#3d3550"; c.textBaseline = "top";
    for (const k in ROOMS) { const r = ROOMS[k]; c.fillText(r.name.toUpperCase(), r.x + 5, r.y + 4); }
    px(14, 50, 44, 22, "#b9a4da"); px(16, 52, 40, 18, "#c8b7e4");
    px(150, 4, 64, 12, "#fff"); px(150, 4, 64, 1, "#adb5bd"); px(154, 8, 22, 1, "#4c6ef5"); px(154, 11, 34, 1, "#e8590c"); px(196, 8, 12, 5, "#51cf66");
    px(112, 100, 24, 12, "#2b2f3a"); px(114, 102, 20, 8, "#38d9a9"); px(116, 104, 10, 1, "#fff"); px(116, 106, 14, 1, "#fff");
    this.rack(236, 92, "QC"); this.rack(274, 92, "PROD");
    [[84, 12], [300, 10], [300, 126], [86, 168], [8, 182], [196, 80]].forEach(([x, y]) => this.plant(x, y));
    px(122, 146, 80, 48, "#b7d6ac"); px(126, 150, 72, 40, "#c3e0b8");
    px(128, 158, 52, 12, "#e8590c"); px(128, 166, 52, 6, "#c2410c"); px(128, 158, 4, 14, "#c2410c"); px(176, 158, 4, 14, "#c2410c");
    px(142, 176, 26, 10, "#a47148"); px(144, 178, 22, 6, "#c08a5a");
    px(298, 140, 14, 22, "#868e96"); px(300, 143, 10, 8, "#74c0fc"); px(300, 154, 10, 3, "#fa5252");
    px(52, 112, 40, 8, "#fff"); px(56, 115, 16, 1, "#e03131"); px(76, 115, 10, 1, "#495057");
    px(70, 176, 22, 14, "#7d5535"); px(72, 178, 18, 4, "#a47148"); px(72, 184, 18, 4, "#a47148");
  }
  private rack(x: number, y: number, label: string) {
    const c = this.ctx;
    this.px(x, y, 18, 24, "#343a40"); this.px(x + 1, y + 1, 16, 22, "#495057");
    for (let i = 0; i < 5; i++) { this.px(x + 3, y + 3 + i * 4, 12, 2, "#212529"); this.px(x + 4, y + 3 + i * 4, 2, 2, "#6c757d"); }
    c.font = "bold 4.4px ui-monospace,Consolas,monospace"; c.fillStyle = "#212529"; c.textBaseline = "top";
    c.fillText(label, x + 9 - c.measureText(label).width / 2, y - 6);
  }
  private drawDesk(p: Person, time: number) {
    const [x, y] = p.desk, px = this.px.bind(this);
    px(x + 9, y + 2, 7, 10, "#343a40");
    this.drawPerson(p, time, true);
    px(x, y, 24, 6, "#c08a5a"); px(x, y + 6, 24, 4, "#8a5d3b"); px(x, y, 24, 1, "#d9a674");
    px(x + 7, y + 3, 10, 2, "#e9ecef"); px(x + 16, y - 6, 8, 6, "#212529");
    const scr = p.state === "running" ? (Math.floor(time * 5) % 2 ? "#69db7c" : "#38d9a9") : p.state === "done" ? "#74c0fc" : p.state === "failed" ? "#ff8787" : "#495057";
    px(x + 17, y - 5, 6, 4, scr); px(x + 19, y, 2, 2, "#212529");
    if (p.state === "running") for (let i = 0; i < 3; i++) { const t = (time * 1.2 + i / 3) % 1; px(x + 17 + i * 2, y - 6 - t * 10, 1, 1, `rgba(56,217,169,${(1 - t).toFixed(2)})`); }
  }
  private drawPerson(p: Person, time: number, seated: boolean) {
    const px = this.px.bind(this), x = Math.round(p.x), y = Math.round(p.y);
    const step = p.moving ? Math.floor(time * 10) % 2 : 0, bob = p.moving && step ? 1 : 0;
    if (!seated) px(x - 4, y - 1, 8, 2, "rgba(0,0,0,.2)");
    px(x - 3, y - 4, 2, 4 - (step ? 1 : 0), "#2b2f3a"); px(x + 1, y - 4, 2, 4 - (step ? 0 : 1), "#2b2f3a");
    px(x - 3, y - 10 - bob, 6, 6, p.shirt);
    if (p.id === "orchestrator") px(x, y - 10 - bob, 1, 5, "#fff");
    const typing = seated && p.state === "running" && Math.floor(time * 10) % 2;
    if (p.moving) { px(x - 4, y - 9 - bob + (step ? 1 : 0), 1, 4, p.skin); px(x + 3, y - 9 - bob + (step ? 0 : 1), 1, 4, p.skin); }
    else if (seated) { px(x - 4, y - 8, 1, 4, p.skin); px(x + 3, y - 8, 1, 4, p.skin); px(typing ? x - 3 : x - 1, y - 4, 2, 1, p.skin); px(typing ? x + 1 : x + 2, y - 4, 2, 1, p.skin); }
    else { px(x - 4, y - 9, 1, 4, p.skin); px(x + 3, y - 9, 1, 4, p.skin); }
    px(x - 3, y - 15 - bob, 6, 5, p.skin); px(x - 3, y - 16 - bob, 6, 2, p.hair); px(x - 4, y - 15 - bob, 1, 3, p.hair); px(x + 3, y - 15 - bob, 1, 3, p.hair);
    px(x - 2, y - 13 - bob, 1, 1, "#222"); px(x + 1, y - 13 - bob, 1, 1, "#222");
    if (p.cap) px(x - 4, y - 17 - bob, 8, 2, "#e03131");
    if (p.carry) this.paper(x + 4, y - 12 - bob);
  }
  private drawCat(time: number) {
    const c = this.cat, px = this.px.bind(this), x = Math.round(c.x), y = Math.round(c.y), step = c.moving ? Math.floor(time * 8) % 2 : 0;
    px(x - 5, y - 1, 10, 2, "rgba(0,0,0,.18)");
    if (c.sleep && !c.moving) { px(x - 4, y - 4, 8, 4, "#e8a35a"); px(x - 4, y - 4, 3, 3, "#d9822b"); px(x + 2, y - 3, 3, 1, "#e8a35a"); return; }
    px(x - 4, y - 5, 8, 3, "#e8a35a"); px(x + 3, y - 8, 4, 4, "#e8a35a"); px(x + 3, y - 9, 1, 1, "#d9822b"); px(x + 6, y - 9, 1, 1, "#d9822b"); px(x + 5, y - 7, 1, 1, "#222");
    px(x - 4, y - 2, 1, 2 - step, "#d9822b"); px(x - 1, y - 2, 1, 1 + step, "#d9822b"); px(x + 2, y - 2, 1, 2 - step, "#d9822b");
    const sw = Math.round(Math.sin(time * 5)); px(x - 6, y - 8 + sw, 1, 4, "#e8a35a"); px(x - 5, y - 5, 1, 1, "#e8a35a");
  }
  private bubble(text: string, cx: number, y: number) {
    const c = this.ctx, px = this.px.bind(this);
    c.font = "4.6px ui-monospace,Consolas,monospace";
    const w = Math.ceil(c.measureText(text).width) + 6, x = Math.round(Math.min(W - w - 2, Math.max(2, cx - w / 2)));
    px(x, y - 8, w, 9, "#fff"); px(x + 2, y + 1, 3, 2, "#fff"); px(x, y - 8, w, 1, "#868e96"); px(x, y, w, 1, "#868e96");
    c.fillStyle = "#212529"; c.textBaseline = "top"; c.fillText(text, x + 3, y - 6);
  }
  private chip(p: Person) {
    const c = this.ctx, px = this.px.bind(this);
    c.font = "bold 4.4px ui-monospace,Consolas,monospace";
    const task = p.task ? (p.task.length > 17 ? p.task.slice(0, 16) + "…" : p.task) : "";
    const w = Math.max(c.measureText(p.label).width + 10, task ? c.measureText(task).width + 6 : 0) + 2;
    const x = Math.round(p.desk[0] + 12 - w / 2), y = p.desk[1] + 11;
    px(x, y, w, task ? 12 : 8, "rgba(255,255,255,.88)"); px(x + 2, y + 2, 3, 3, STATUS_COLORS[p.state]);
    c.fillStyle = "#212529"; c.textBaseline = "top"; c.fillText(p.label, x + 7, y + 1.6);
    if (task) { c.font = "4px ui-monospace,Consolas,monospace"; c.fillStyle = "#495057"; c.fillText(task, x + 3, y + 7); }
  }
  private draw(t: number) {
    const c = this.ctx;
    c.setTransform(S, 0, 0, S, 0, 0); c.imageSmoothingEnabled = false;
    this.drawRooms();
    [...this.people].sort((a, b) => a.desk[1] - b.desk[1]).forEach((p) => this.drawDesk(p, t));
    [this.boy, this.cat as unknown as Person].sort((a, b) => a.y - b.y).forEach((e) => (e === (this.cat as unknown) ? this.drawCat(t) : this.drawPerson(e, t, false)));
    this.people.forEach((p) => this.chip(p));
    for (const f of this.flights) { const k = Math.min(1, f.t / f.dur), e = k * k * (3 - 2 * k); this.paper(f.fx + (f.tx - f.fx) * e, f.fy + (f.ty - f.fy) * e - Math.sin(k * Math.PI) * 8); }
    for (const p of [...this.people, this.boy]) if (p.say) this.bubble(p.say, p.x, p.y - 20);
    if (this.selected && this.byId[this.selected]) { const p = this.byId[this.selected]; this.px(p.x - 6, p.y - 19, 12, 1, "#fff"); this.px(p.x - 6, p.y - 19, 1, 3, "#fff"); this.px(p.x + 5, p.y - 19, 1, 3, "#fff"); }
  }
}
