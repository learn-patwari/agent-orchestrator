// Cartoon office scene for the Agents page. Drawing and walking only: it never invents work.
// People sit at their desks; the office boy walks only when the app reports a real hand-off.
// Look and layout follow the design in docs/design/office-view-mockup.html.

export type TeamRow = { role: string; description?: string; model?: string | null; state: string; task: string | null; steps: number };
export type Deployed = { task_id: string; name: string; version: string; at: number };
export type Environments = { qc: Deployed | null; prod: Deployed | null };
export type CharStyle = "robots" | "people" | "animals" | "heroes" | "stars_in" | "stars_world";
export const CHAR_STYLES: { id: CharStyle; label: string }[] = [
  { id: "robots", label: "Robots" }, { id: "people", label: "People" }, { id: "animals", label: "Animals" }, { id: "heroes", label: "Heroes" },
  { id: "stars_in", label: "Stars (Indian-inspired)" }, { id: "stars_world", label: "Stars (world-inspired)" },
];
export type Handoff = { id: string; from: string; to: string; label: string; at: number };

type Pt = [number, number];
type Status = "idle" | "running" | "done" | "failed";
type Mover = {
  x: number; y: number; path: Pt[]; moving: boolean; res: (() => void) | null; speed: number; say: string; sayT: number;
};
type Person = Mover & {
  id: string; label: string; desk: Pt; home: Pt; front: Pt; shirt: string; hair: string; skin: string;
  state: Status; task: string | null; dummy: boolean; carry: string | null; acc: string; phase: number; capColor: string; look: number;
};

const W = 320, H = 200;
const OUT = "#3b3150";
const FONT = '"Baloo 2","Comic Sans MS","Chalkboard SE","Trebuchet MS",system-ui,sans-serif';
/** Lighten (amt > 0) or darken (amt < 0) a #rrggbb colour. */
function shade(hex: string, amt: number): string {
  const n = parseInt(hex.slice(1), 16), f = (v: number) => Math.max(0, Math.min(255, Math.round(v + (amt < 0 ? v : 255 - v) * amt)));
  return `rgb(${f(n >> 16)},${f((n >> 8) & 255)},${f(n & 255)})`;
}
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
const SEATS: { id: string; label: string; desk: Pt; shirt: string; hair: string; acc: string; dummy?: boolean }[] = [
  { id: "orchestrator", label: "orchestrator", desk: [36, 34], shirt: "#364fc7", hair: "#222", acc: "tie" },
  { id: "architect", label: "architect", desk: [110, 28], shirt: "#f59f00", hair: "#6b3f1d", acc: "glasses" },
  { id: "developer_1", label: "developer 1", desk: [152, 28], shirt: "#2f9e44", hair: "#3b2412", acc: "phones" },
  { id: "developer_2", label: "developer 2", desk: [194, 28], shirt: "#12b886", hair: "#1b1b1b", acc: "phones" },
  { id: "cicd", label: "ci/cd", desk: [236, 28], shirt: "#1098ad", hair: "#7a4b21", acc: "cap" },
  { id: "tester", label: "tester", desk: [168, 86], shirt: "#ae3ec9", hair: "#d9a441", acc: "bigglasses" },
  { id: "incident_reporter", label: "incident rep", desk: [14, 132], shirt: "#e8590c", hair: "#222", acc: "hardhat" },
  { id: "production_support", label: "prod support", desk: [58, 132], shirt: "#5c7cfa", hair: "#a0522d", acc: "headset" },
  { id: "technical_writer", label: "tech writer", desk: [278, 28], shirt: "#7048e8", hair: "#4a2c17", acc: "pencil" },
  { id: "receptionist", label: "receptionist", desk: [262, 146], shirt: "#e64980", hair: "#5c3317", acc: "bun", dummy: true },
];
type Animal = { fur: string; muz: string; ear: "round" | "point" | "long" | "floppy" | "tuft" | "none"; inner: string; mark?: string; nose?: string };
const ANIMALS: Record<string, Animal> = {
  owl: { fur: "#a9794f", muz: "#f3e3c3", ear: "tuft", inner: "#8a5a35", mark: "owl" },
  fox: { fur: "#f08a3c", muz: "#fff4e5", ear: "point", inner: "#3b3150", mark: "fox" },
  panda: { fur: "#f7f7f7", muz: "#fff", ear: "round", inner: "#2b2f3a", mark: "panda" },
  bunny: { fur: "#f6eef0", muz: "#fff", ear: "long", inner: "#ffb3c6", nose: "#ff7a9c" },
  dog: { fur: "#c98c52", muz: "#f6dfc0", ear: "floppy", inner: "#8a5a2b" },
  tiger: { fur: "#f6a21e", muz: "#fff3d6", ear: "round", inner: "#fff", mark: "tiger" },
  bear: { fur: "#9b6a43", muz: "#d9b48a", ear: "round", inner: "#6e4a2e" },
  mouse: { fur: "#b8bfcc", muz: "#e9edf5", ear: "round", inner: "#ffb3c6", nose: "#ff7a9c" },
  frog: { fur: "#6fcf6f", muz: "#c8f2b5", ear: "none", inner: "#4fae4f", mark: "frog" },
  koala: { fur: "#9aa3b2", muz: "#cfd4dc", ear: "round", inner: "#e9edf5", mark: "koala" },
};
const ANIMAL_OF: Record<string, string> = { orchestrator: "owl", architect: "fox", developer_1: "panda", developer_2: "bunny", cicd: "dog", tester: "tiger", incident_reporter: "bear", production_support: "mouse", technical_writer: "frog", receptionist: "koala", "office boy": "mouse" };
const HERO_OF: Record<string, string> = { orchestrator: "wizard", architect: "knight", developer_1: "ninja", developer_2: "pirate", cicd: "astronaut", tester: "detective", incident_reporter: "firefighter", production_support: "superhero", technical_writer: "scholar", receptionist: "princess", "office boy": "courier" };
// Fictional "star" archetypes (action star, cricket captain, singer...). No real person is drawn or named.
const STAR_IN: Record<string, string> = { orchestrator: "director", architect: "shades", developer_1: "cricket", developer_2: "flowers", cicd: "singer", tester: "shades", incident_reporter: "cricket", production_support: "singer", technical_writer: "director", receptionist: "flowers", "office boy": "cricket" };
const STAR_WORLD: Record<string, string> = { orchestrator: "tophat", architect: "aviator", developer_1: "rock", developer_2: "headband", cicd: "aviator", tester: "director", incident_reporter: "headband", production_support: "rock", technical_writer: "tophat", receptionist: "divaglasses", "office boy": "headband" };
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
  private env: Environments = { qc: null, prod: null };
  private working = false;
  private raf = 0;
  private last = 0;
  private clock = 0;
  private style: CharStyle = "robots";
  private S = 3;
  private ro: ResizeObserver | null = null;
  private catTimer = 2;
  paused = false;
  selected: string | null = null;
  onSelect: (role: string) => void = () => {};

  constructor(private cv: HTMLCanvasElement, reduceMotion = false) {
    this.fit();
    if (typeof ResizeObserver !== "undefined") { this.ro = new ResizeObserver(() => this.fit()); this.ro.observe(cv); }
    this.ctx = cv.getContext("2d") as CanvasRenderingContext2D;
    this.paused = reduceMotion;
    SEATS.forEach((s, i) => {
      const home: Pt = [s.desk[0] + 12, s.desk[1] + 8];
      const p: Person = {
        id: s.id, label: s.label, desk: s.desk, home, front: [s.desk[0] + 12, s.desk[1] + 24], shirt: s.shirt, hair: s.hair,
        skin: SKIN[i % 4], state: "idle", task: s.dummy ? "waiting" : null, dummy: !!s.dummy, carry: null, acc: s.acc, phase: i * 1.7, capColor: "#1098ad", look: i % 4,
        x: home[0], y: home[1], path: [], moving: false, res: null, speed: 0, say: "", sayT: 0,
      };
      this.people.push(p); this.byId[s.id] = p;
    });
    this.boy = { id: "office boy", label: "office boy", desk: [0, 0], home: [214, 160], front: [214, 160], shirt: "#868e96", hair: "#222",
      skin: SKIN[1], state: "idle", task: null, dummy: true, carry: null, acc: "cap", phase: 0.5, capColor: "#e03131", look: 2,
      x: 214, y: 160, path: [], moving: false, res: null, speed: 95, say: "", sayT: 0 };
    this.cat = { x: 160, y: 150, path: [], moving: false, res: null, speed: 34, say: "", sayT: 0, sleep: false };
    cv.addEventListener("click", this.click);
    this.raf = requestAnimationFrame(this.frame);
  }

  /** Match the canvas to its on-screen size so lines stay sharp on any screen. */
  private fit() {
    const dpr = window.devicePixelRatio || 1, w = this.cv.clientWidth || W;
    this.S = Math.max(2, Math.min(6, (w * dpr) / W));
    this.cv.width = Math.round(W * this.S); this.cv.height = Math.round(H * this.S);
  }

  destroy() { this.ro?.disconnect(); cancelAnimationFrame(this.raf); this.cv.removeEventListener("click", this.click); }

  /** Real state from the app. Desks only change colour, bubble and screen; nobody moves because of it. */
  setTeam(rows: TeamRow[]) {
    for (const r of rows) {
      const p = this.byId[r.role]; if (!p || p.dummy) continue;
      p.state = toStatus(r.state); p.task = r.task;
    }
  }

  /** Characters are only a look: robots, people, animals or heroes (generic costumes). */
  setStyle(style: CharStyle) { this.style = style; }

  /** What is deployed to QC and production. A rack lights up and shows its version only when the app says so. */
  setEnvironments(env: Environments) { this.env = env; }

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
    const hit = this.people.filter((p) => Math.abs(lx - p.x) < 9 && ly > p.y - 24 && ly < p.y + 4).sort((a, b) => b.y - a.y)[0];
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
    if (!this.paused) { this.update(dt); this.clock += dt; }
    this.draw(this.clock);
    this.raf = requestAnimationFrame(this.frame);
  };

  // ---------- drawing: cartoon style (smooth vector shapes, thick outlines, big heads) ----------
  private box(x: number, y: number, w: number, h: number, r: number, fill: string | null, lw = 0.9, stroke = OUT) {
    const c = this.ctx; c.beginPath(); c.roundRect(x, y, w, h, r);
    if (fill) { c.fillStyle = fill; c.fill(); }
    if (lw) { c.lineWidth = lw; c.strokeStyle = stroke; c.stroke(); }
  }
  private ball(x: number, y: number, rx: number, ry: number, fill: string | null, lw = 0.9, rot = 0, stroke = OUT) {
    const c = this.ctx; c.beginPath(); c.ellipse(x, y, rx, ry, rot, 0, Math.PI * 2);
    if (fill) { c.fillStyle = fill; c.fill(); }
    if (lw) { c.lineWidth = lw; c.strokeStyle = stroke; c.stroke(); }
  }
  private text(t: string, x: number, y: number, size: number, color: string, align: CanvasTextAlign = "left") {
    const c = this.ctx; c.font = `bold ${size}px ${FONT}`; c.fillStyle = color; c.textAlign = align; c.textBaseline = "middle"; c.fillText(t, x, y);
  }
  private paper(x: number, y: number, rot = -0.2) {
    const c = this.ctx; c.save(); c.translate(x, y); c.rotate(rot);
    this.box(-3, -4, 6, 8, 1, "#fff", 0.7);
    c.fillStyle = "#9aa5b8"; c.fillRect(-1.8, -2.4, 3.6, 0.7); c.fillRect(-1.8, -0.8, 3.6, 0.7); c.fillRect(-1.8, 0.8, 2.2, 0.7);
    c.restore();
  }
  private plant(x: number, y: number, t: number) {
    const sway = Math.sin(t * 1.3 + x) * 0.07;
    this.box(x + 1, y + 6, 5.4, 4.2, 1.2, "#c46f3a", 0.8);
    this.ball(x + 3.7, y + 6.2, 3.2, 0.9, "#7a4a2a", 0);
    this.ball(x + 1.4, y + 3.4, 1.6, 3.6, "#3fae5a", 0.8, -0.7 + sway);
    this.ball(x + 6, y + 3.4, 1.6, 3.6, "#3fae5a", 0.8, 0.7 + sway);
    this.ball(x + 3.7, y + 2.2, 1.8, 4.2, "#5fcf78", 0.8, sway);
  }
  private rug(x: number, y: number, w: number, h: number, a: string, b: string) {
    this.box(x, y, w, h, 5, a, 0.9); this.box(x + 2.2, y + 2.2, w - 4.4, h - 4.4, 3.5, b, 0);
    this.ctx.save(); this.ctx.setLineDash([1.6, 1.6]); this.box(x + 3.4, y + 3.4, w - 6.8, h - 6.8, 2.5, null, 0.5, "rgba(255,255,255,.85)"); this.ctx.restore();
  }
  private drawRooms(t: number) {
    const c = this.ctx;
    c.lineJoin = "round"; c.lineCap = "round";
    const g = c.createLinearGradient(0, 0, 0, H); g.addColorStop(0, "#f3ead6"); g.addColorStop(1, "#e3d6bd");
    c.fillStyle = g; c.fillRect(0, 0, W, H);
    for (const k in ROOMS) {
      const r = ROOMS[k];
      this.box(r.x + 1, r.y + 2.6, r.w, r.h, 6, "rgba(70,50,90,.18)", 0);
      c.save(); c.beginPath(); c.roundRect(r.x, r.y, r.w, r.h, 6); c.clip();
      c.fillStyle = r.c; c.fillRect(r.x, r.y, r.w, r.h);
      c.fillStyle = "rgba(255,255,255,.28)"; for (let yy = r.y + 17; yy < r.y + r.h; yy += 9) c.fillRect(r.x, yy, r.w, 0.7);
      c.fillStyle = shade(r.c, -0.14); c.fillRect(r.x, r.y, r.w, 10);
      c.fillStyle = shade(r.c, -0.3); c.fillRect(r.x, r.y + 9, r.w, 1.4);
      c.restore();
      this.box(r.x, r.y, r.w, r.h, 6, null, 1.4);
      const tw = r.name.length * 2.5 + 7;
      this.box(r.x + 4, r.y + 2.4, tw, 6.2, 3.1, "rgba(255,255,255,.92)", 0.7);
      this.text(r.name, r.x + 4 + tw / 2, r.y + 5.6, 4.1, "#4a3d63", "center");
    }
    GAPS.forEach((gp) => { c.fillStyle = "#e9dec7"; c.fillRect(gp[0] - 0.5, gp[1], gp[2] + 1, gp[3]); });

    // manager: rug and a framed star
    this.rug(14, 50, 46, 24, "#a98bd6", "#c9b4ec");
    this.box(64, 2.4, 14, 7, 1, "#fff6d6", 0.8); this.text("★", 71, 6.2, 5.6, "#f2a900", "center");
    // engineering: planning board
    this.box(150, 3.5, 66, 14, 2, "#fff", 1);
    this.box(152, 5.5, 62, 1.6, 0.8, "#4c6ef5", 0); this.box(152, 9.5, 40, 1.6, 0.8, "#f76707", 0); this.box(152, 13, 28, 1.6, 0.8, "#12b886", 0);
    this.box(196, 8.5, 14, 6, 1.4, "#ffe066", 0.6);
    // QC: checklist board
    this.box(112, 98, 30, 17, 2.5, "#fff", 1);
    for (let i = 0; i < 3; i++) { this.text("✓", 117, 102 + i * 4.8, 4.6, "#2f9e44", "center"); this.box(121, 101 + i * 4.8, 15 - i * 3, 1.4, 0.7, "#ced4da", 0); }
    // servers
    this.rack(236, 92, "QC", this.env.qc, t); this.rack(274, 92, "PROD", this.env.prod, t);
    // plants
    [[84, 12], [307, 11], [300, 124], [84, 168], [8, 182], [196, 80]].forEach(([x, y]) => this.plant(x, y, t));
    // lounge: rug, sofa, coffee table
    this.rug(122, 146, 80, 48, "#8fc98a", "#b4e2ad");
    this.box(128, 154, 52, 5, 3, "#d9660f", 0.9);
    this.box(126, 157, 56, 15, 5, "#f57c2b", 1); this.box(132, 160, 20, 9, 3, "#ff9a4d", 0.5); this.box(155, 160, 20, 9, 3, "#ff9a4d", 0.5);
    this.box(142, 177, 28, 10, 4, "#b98253", 0.9); this.box(144, 178.5, 24, 3.5, 2, "#d2a06e", 0);
    this.ball(150, 175.5, 2.2, 2.2, "#fff", 0.7); this.ball(150, 175.5, 1.2, 1.2, "#6f4e37", 0);
    this.box(158, 176, 7, 3.5, 0.8, "#e64980", 0.6);
    // water cooler
    this.box(300, 148, 12, 14, 2.5, "#d5dce8", 0.9);
    this.box(301, 138, 10, 12, 4, "rgba(116,192,252,.85)", 0.9); this.box(302.5, 140, 2, 6, 1, "rgba(255,255,255,.8)", 0);
    this.ball(303.5, 152.5, 0.9, 0.9, "#fa5252", 0); this.ball(307.5, 152.5, 0.9, 0.9, "#4dabf7", 0);
    // support: wall clock (real time, just decoration) and bookshelf
    const d = new Date(), cx = 72, cy = 14 + 98;
    this.ball(cx, cy, 5.4, 5.4, "#fff", 1);
    c.strokeStyle = OUT; c.lineWidth = 0.8; c.beginPath(); c.moveTo(cx, cy); c.lineTo(cx + Math.sin((d.getMinutes() / 60) * 6.283) * 4, cy - Math.cos((d.getMinutes() / 60) * 6.283) * 4); c.stroke();
    c.lineWidth = 1; c.beginPath(); c.moveTo(cx, cy); c.lineTo(cx + Math.sin(((d.getHours() % 12) / 12) * 6.283) * 2.6, cy - Math.cos(((d.getHours() % 12) / 12) * 6.283) * 2.6); c.stroke();
    this.box(66, 174, 28, 18, 2.5, "#9b6a43", 1); this.box(68, 176, 24, 6, 1, "#7b5232", 0); this.box(68, 184, 24, 6, 1, "#7b5232", 0);
    ["#fa5252", "#4dabf7", "#ffd43b", "#51cf66", "#be4bdb", "#ff922b"].forEach((col, i) => { this.box(69 + i * 3.7, 177 - (i % 2) * 0.6, 2.8, 4.6 + (i % 2) * 0.6, 0.6, col, 0.4); this.box(69 + i * 3.7, 185, 2.8, 4.4 - (i % 3) * 0.4, 0.6, col, 0.4); });
  }
  private rack(x: number, y: number, label: string, dep: Deployed | null, t: number) {
    this.box(x, y, 18, 24, 2.5, "#505a6e", 1);
    for (let i = 0; i < 4; i++) {
      this.box(x + 2, y + 3 + i * 5, 14, 4, 1, "#2b303c", 0.5);
      const on = dep ? (Math.sin(t * 5 + i * 1.7 + x) > -0.3 ? "#69db7c" : "#2f9e44") : "#6c7488";
      this.ball(x + 4.5, y + 5 + i * 5, 0.9, 0.9, on, 0);
      this.box(x + 7, y + 4.4 + i * 5, 7, 1.2, 0.6, dep ? "#4d5668" : "#3a4152", 0);
    }
    this.text(label, x + 9, y - 3.4, 4.6, "#3b3150", "center");
    this.text(dep ? dep.version || "live" : "empty", x + 9, y + 28, 4.2, dep ? "#2b8a3e" : "#7a7390", "center");
  }
  private drawDesk(p: Person, t: number) {
    const [x, y] = p.desk;
    // chair behind the person
    this.box(x + 6, y - 1, 12, 13, 4, "#454d63", 0.9);
    this.drawPerson(p, t, true);
    // desk
    this.box(x, y + 1, 24, 6, 2.2, "#e0a96d", 1); this.box(x + 1, y + 6, 22, 7, 2, "#b87e4c", 1);
    this.box(x + 3, y + 8.4, 8, 2.6, 1, "#9a6538", 0);
    this.box(x + 6, y + 2.4, 10, 2.4, 1, "#f1f3f8", 0.6);
    this.ball(x + 2.6, y + 2.6, 1.7, 1.7, "#fff", 0.7); this.ball(x + 4.1, y + 2.5, 0.5, 0.8, null, 0.5);
    // monitor
    this.box(x + 16, y - 9, 11, 9, 1.8, "#323a4d", 0.9); this.box(x + 20, y - 0.6, 3, 2, 0.6, "#323a4d", 0.6);
    const sx = x + 17.3, sy = y - 7.7, sw = 8.4, sh = 6.4;
    this.box(sx, sy, sw, sh, 1, p.state === "running" ? "#0d3b2e" : p.state === "done" ? "#3b82f6" : p.state === "failed" ? "#f25f5c" : "#566078", 0);
    const c = this.ctx;
    if (p.state === "running") {
      c.fillStyle = "#69db7c"; for (let i = 0; i < 3; i++) { const w = 2.4 + ((Math.floor(t * 3) + i * 2) % 4); c.fillRect(sx + 1, sy + 1 + i * 1.8, w, 0.9); }
      for (let i = 0; i < 3; i++) { const k = (t * 1.1 + i / 3) % 1; this.ball(x + 21 + Math.sin(k * 6 + i) * 2, y - 11 - k * 9, 0.8, 0.8, `rgba(56,217,169,${(1 - k).toFixed(2)})`, 0); }
    } else if (p.state === "done") this.text("✓", sx + sw / 2, sy + sh / 2 + 0.2, 5.6, "#fff", "center");
    else if (p.state === "failed") this.text("!", sx + sw / 2, sy + sh / 2 + 0.2, 5.6, "#fff", "center");
    else { c.fillStyle = "rgba(255,255,255,.35)"; c.fillRect(sx + 1.2, sy + 1.2, 3, 0.8); }
  }
  private drawPerson(p: Person, t: number, seated: boolean) {
    const c = this.ctx, x = p.x, y = p.y, ph = t * 12 + p.phase, walk = p.moving;
    const bob = walk ? Math.abs(Math.sin(ph)) * 1.3 : Math.sin(t * 2.2 + p.phase) * 0.35;
    const typing = seated && p.state === "running";
    if (!seated) this.ball(x, y, 5.6, 1.7, "rgba(60,40,80,.25)", 0);
    if (!seated) {
      const l1 = walk ? Math.max(0, Math.sin(ph)) * 1.6 : 0, l2 = walk ? Math.max(0, -Math.sin(ph)) * 1.6 : 0;
      this.box(x - 3.3, y - 4.4 - l1, 2.6, 4.4, 1.1, "#3d4770", 0.8); this.box(x + 0.7, y - 4.4 - l2, 2.6, 4.4, 1.1, "#3d4770", 0.8);
      this.ball(x - 2, y - l1, 2, 1.1, "#2b2f3a", 0.6); this.ball(x + 2, y - l2, 2, 1.1, "#2b2f3a", 0.6);
    }
    // arms behind the body
    const swing = walk ? Math.sin(ph) * 2 : 0;
    const hand = (hx: number, hy: number) => this.ball(hx, hy, 1.5, 1.5, this.style === "robots" ? "#e6eaf2" : this.style === "animals" ? ANIMALS[ANIMAL_OF[p.id] ?? "mouse"].fur : p.skin, 0.7);
    c.lineWidth = 2.6; c.strokeStyle = OUT; c.beginPath();
    if (seated) { c.moveTo(x - 4, y - 8 - bob); c.lineTo(x - 3 + (typing ? Math.sin(t * 22) * 0.8 : 0), y - 2.6); c.moveTo(x + 4, y - 8 - bob); c.lineTo(x + 3 + (typing ? Math.cos(t * 22) * 0.8 : 0), y - 2.6); }
    else { c.moveTo(x - 4, y - 8 - bob); c.lineTo(x - 5.3, y - 4.6 + swing - bob); c.moveTo(x + 4, y - 8 - bob); c.lineTo(x + 5.3, y - 4.6 - swing - bob); }
    c.stroke();
    c.lineWidth = 1.4; c.strokeStyle = p.shirt; c.stroke();
    if (seated) { hand(x - 3, y - 2.4); hand(x + 3, y - 2.4); } else { hand(x - 5.3, y - 4.2 + swing - bob); hand(x + 5.3, y - 4.2 - swing - bob); }
    // body
    if (this.style === "heroes" && (HERO_OF[p.id] === "superhero" || HERO_OF[p.id] === "wizard")) { c.beginPath(); c.moveTo(x - 4, y - 9 - bob); c.lineTo(x + 4, y - 9 - bob); c.lineTo(x + 6.4 + Math.sin(t * 6) * (p.moving ? 1.4 : 0.3), y - 0.5); c.lineTo(x - 6.4, y - 0.5); c.closePath(); c.fillStyle = HERO_OF[p.id] === "superhero" ? "#e03131" : "#7048e8"; c.fill(); c.lineWidth = 0.9; c.strokeStyle = OUT; c.stroke(); }
    this.box(x - 4.4, y - 10.4 - bob, 8.8, 8, 3, p.shirt, 0.9);
    this.box(x - 3, y - 9 - bob, 2.4, 3.4, 1.2, "rgba(255,255,255,.2)", 0);
    if (this.style === "robots") {
      this.box(x - 2.8, y - 9 - bob, 5.6, 4.4, 1.4, "#f4f6fb", 0.6);
      if (p.id === "orchestrator") this.text("★", x, y - 6.6 - bob, 4.2, "#f2a900", "center");
      else this.ball(x, y - 6.8 - bob, 1.2, 1.2, STATUS_COLORS[p.state], 0.4);
    } else if (this.style === "animals") this.ball(x, y - 6.4 - bob, 2.6, 2.8, ANIMALS[ANIMAL_OF[p.id] ?? "mouse"].muz, 0);
    else if (this.style === "people" && p.acc === "tie") { c.fillStyle = "#e03131"; c.beginPath(); c.moveTo(x - 1, y - 10 - bob); c.lineTo(x + 1, y - 10 - bob); c.lineTo(x + 0.8, y - 5 - bob); c.lineTo(x, y - 4 - bob); c.lineTo(x - 0.8, y - 5 - bob); c.fill(); }
    if (p.carry) this.paper(x + 6.4, y - 8 - bob);
    this.drawHead(p, x, y - 16.6 - bob, t, seated);
  }
  private drawHeadRobot(p: Person, x: number, y: number, t: number, seated: boolean) {
    const c = this.ctx, st = seated ? p.state : "idle", k = p.look, sc = STATUS_COLORS[st];
    // antenna (its light shows the agent's state)
    const pulse = st === "running" ? 1 + Math.sin(t * 8 + p.phase) * 0.28 : 1;
    c.lineWidth = 1.1; c.strokeStyle = OUT;
    if (k === 1) { c.beginPath(); c.moveTo(x - 3, y - 5); c.lineTo(x - 5, y - 10); c.moveTo(x + 3, y - 5); c.lineTo(x + 5, y - 10); c.stroke(); this.ball(x - 5, y - 10.4, 1.6 * pulse, 1.6 * pulse, sc, 0.8); this.ball(x + 5, y - 10.4, 1.4, 1.4, "#fff", 0.8); }
    else if (k === 2) { this.box(x - 2.6, y - 8.8, 5.2, 4, 1.6, p.shirt, 0.9); this.ball(x, y - 6.8, 1.1 * pulse, 1.1 * pulse, sc, 0); }
    else { c.beginPath(); c.moveTo(x, y - 5); c.lineTo(x, y - 9.4); c.stroke(); this.ball(x, y - 10, 2 * pulse, 2 * pulse, sc, 0.9); }
    // ear nubs and head
    this.box(x - 7.4, y - 1.6, 2.2, 4, 1, p.shirt, 0.8); this.box(x + 5.2, y - 1.6, 2.2, 4, 1, p.shirt, 0.8);
    if (k === 0) this.box(x - 6.2, y - 5.2, 12.4, 10.6, 3.8, "#f4f6fb", 1);
    else if (k === 1) this.box(x - 6.4, y - 4.8, 12.8, 9.8, 4.8, "#f4f6fb", 1);
    else if (k === 2) this.box(x - 5.8, y - 5.4, 11.6, 10.8, 2.6, "#f4f6fb", 1);
    else this.ball(x, y, 6.6, 5.4, "#f4f6fb", 1);
    this.box(x - 4.6, y - 4.2, 3, 1.4, 0.7, "rgba(255,255,255,.9)", 0);
    // dark visor with glowing face
    this.box(x - 4.9, y - 2.8, 9.8, 6, 2.6, "#1f2a3d", 0.8);
    const blink = (t * 0.9 + p.phase) % 4 < 0.13, ex = st === "running" ? 0.7 : 0, glow = st === "failed" ? "#ff8787" : "#6ee7b7";
    c.lineWidth = 0.9; c.strokeStyle = glow; c.fillStyle = glow;
    for (const dx of [-2.2, 2.2]) {
      if (st === "failed") { c.beginPath(); c.moveTo(x + dx - 0.9, y - 1.8); c.lineTo(x + dx + 0.9, y - 0.2); c.moveTo(x + dx + 0.9, y - 1.8); c.lineTo(x + dx - 0.9, y - 0.2); c.stroke(); }
      else if (st === "done") { c.beginPath(); c.arc(x + dx, y - 0.4, 1, Math.PI * 1.1, Math.PI * 1.9); c.stroke(); }
      else if (blink) { c.beginPath(); c.moveTo(x + dx - 0.9, y - 1); c.lineTo(x + dx + 0.9, y - 1); c.stroke(); }
      else if (k === 2) { c.beginPath(); c.arc(x + dx * 0.5 + ex, y - 1, 1.2, 0, 6.283); c.fill(); }
      else { c.beginPath(); c.roundRect(x + dx - 0.9 + ex, y - 2, 1.8, 2.2, 0.9); c.fill(); }
    }
    c.beginPath();
    if (st === "failed") c.arc(x, y + 3.2, 1.1, Math.PI * 1.15, Math.PI * 1.85);
    else if (st === "running") { c.moveTo(x - 1, y + 1.7); c.lineTo(x + 1, y + 1.7); }
    else c.arc(x, y + 0.9, 1.5, 0.15 * Math.PI, 0.85 * Math.PI);
    c.stroke();
    if (st === "failed") { this.ball(x + 6.2, y - 4 + ((t * 2) % 1.5), 0.8, 1.2, "#74c0fc", 0.4); }
    // accessories
    if (p.acc === "phones" || p.acc === "headset") {
      c.lineWidth = 1; c.strokeStyle = "#343a40"; c.beginPath(); c.arc(x, y - 0.4, 7, Math.PI * 1.02, Math.PI * 1.98); c.stroke();
      const hc = p.acc === "phones" ? "#ff6b6b" : "#4c6ef5";
      this.box(x - 8.8, y - 1.6, 2.6, 4.6, 1.2, hc, 0.7); this.box(x + 6.2, y - 1.6, 2.6, 4.6, 1.2, hc, 0.7);
      if (p.acc === "headset") { c.lineWidth = 0.8; c.beginPath(); c.moveTo(x - 7.6, y + 3); c.quadraticCurveTo(x - 6, y + 6.4, x - 2.6, y + 5.4); c.stroke(); this.ball(x - 2.6, y + 5.4, 0.9, 0.9, "#343a40", 0); }
    }
    if (p.acc === "cap" || p.acc === "hardhat") {
      const col = p.acc === "hardhat" ? "#ffa94d" : p.capColor;
      c.beginPath(); c.arc(x, y - 3.4, 6.2, Math.PI, 0); c.closePath(); c.fillStyle = col; c.fill(); c.lineWidth = 0.9; c.strokeStyle = OUT; c.stroke();
      if (p.acc === "cap") this.box(x + 1, y - 4, 7, 1.7, 0.9, shade(col, -0.15), 0.8); else this.box(x - 7, y - 4.2, 14, 1.7, 0.8, shade(col, -0.1), 0.8);
    }
    if (p.acc === "pencil") { c.save(); c.translate(x + 7.4, y - 2.4); c.rotate(0.5); this.box(-0.6, -3.4, 1.3, 5.6, 0.5, "#ffd43b", 0.5); this.box(-0.6, -3.9, 1.3, 1, 0.4, "#ff8787", 0.4); c.restore(); }
  }
  /** Pick the head for the current character style. */
  private drawHead(p: Person, x: number, y: number, t: number, seated: boolean) {
    if (this.style === "animals") return this.drawHeadAnimal(p, x, y, t, seated);
    if (this.style === "people") return this.drawHeadHuman(p, x, y, t, seated, true);
    if (this.style === "heroes" || this.style === "stars_in" || this.style === "stars_world") { this.drawHeadHuman(p, x, y, t, seated, false); return this.drawCostume(p, x, y); }
    return this.drawHeadRobot(p, x, y, t, seated);
  }
  /** Eyes, cheeks and mouth that follow the agent's state. Shared by people, animals and heroes. */
  private face(p: Person, x: number, st: string, t: number, o: { dx: number; ey: number; my: number; mw?: number }) {
    const c = this.ctx, blink = (t * 0.9 + p.phase) % 4 < 0.13, look = st === "running" ? 0.5 : 0;
    for (const dx of [-o.dx, o.dx]) {
      c.lineWidth = 0.8; c.strokeStyle = OUT;
      if (blink) { c.beginPath(); c.moveTo(x + dx - 1, o.ey); c.lineTo(x + dx + 1, o.ey); c.stroke(); }
      else if (st === "done") { c.beginPath(); c.arc(x + dx, o.ey + 0.5, 1.1, Math.PI * 1.1, Math.PI * 1.9); c.stroke(); }
      else { this.ball(x + dx, o.ey, 1, 1.35, "#fff", 0.5); this.ball(x + dx + look, o.ey + 0.1, 0.62, 0.95, "#222", 0); this.ball(x + dx + look + 0.25, o.ey - 0.35, 0.22, 0.22, "#fff", 0); }
    }
    c.lineWidth = 0.8; c.strokeStyle = OUT; c.beginPath();
    const mw = o.mw ?? 1.7;
    if (st === "failed") {
      c.arc(x, o.my + 1.6, mw * 0.9, Math.PI * 1.15, Math.PI * 1.85); c.stroke();
      c.beginPath(); c.moveTo(x - o.dx - 1.2, o.ey - 1.6); c.lineTo(x - o.dx + 1, o.ey - 2.3); c.moveTo(x + o.dx + 1.2, o.ey - 1.6); c.lineTo(x + o.dx - 1, o.ey - 2.3); c.stroke();
      this.ball(x + o.dx + 3.4, o.ey - 3 + ((t * 2) % 1.5), 0.8, 1.2, "#74c0fc", 0.4);
    } else if (st === "running") { c.moveTo(x - 1.1, o.my); c.lineTo(x + 1.1, o.my); c.stroke(); }
    else if (st === "done") { c.arc(x, o.my - 0.6, mw, 0.05 * Math.PI, 0.95 * Math.PI); c.closePath(); c.fillStyle = "#fff"; c.fill(); c.stroke(); }
    else { c.arc(x, o.my - 0.6, mw, 0.15 * Math.PI, 0.85 * Math.PI); c.stroke(); }
  }
  private drawHeadHuman(p: Person, x: number, y: number, t: number, seated: boolean, acc: boolean) {
    const c = this.ctx, st = seated ? p.state : "idle", ey = y + 0.9;
    this.ball(x - 5.4, y + 0.8, 1.2, 1.5, p.skin, 0.7); this.ball(x + 5.4, y + 0.8, 1.2, 1.5, p.skin, 0.7);
    this.ball(x, y, 5.9, 5.6, p.skin, 1);
    c.beginPath(); c.arc(x, y - 0.2, 6, Math.PI * 1.02, Math.PI * 1.98);
    c.quadraticCurveTo(x + 3.4, y - 3.4, x + 0.6, y - 2.4); c.quadraticCurveTo(x - 2.6, y - 3.4, x - 5.9, y - 0.4); c.closePath();
    c.fillStyle = p.hair; c.fill(); c.lineWidth = 0.9; c.strokeStyle = OUT; c.stroke();
    if (acc && p.acc === "bun") { this.ball(x + 0.5, y - 7.6, 2.4, 2.2, p.hair, 0.9); this.box(x - 1.6, y - 6.2, 4.2, 1.2, 0.6, "#ff6b9d", 0.5); }
    this.face(p, x, st, t, { dx: 2.2, ey, my: y + 3.4 });
    this.ball(x - 3.7, y + 2.7, 1.2, 0.8, "rgba(255,105,135,.4)", 0); this.ball(x + 3.7, y + 2.7, 1.2, 0.8, "rgba(255,105,135,.4)", 0);
    if (!acc) return;
    if (p.acc === "glasses" || p.acc === "bigglasses") {
      const r = p.acc === "bigglasses" ? 2 : 1.7; c.lineWidth = 0.7; c.strokeStyle = p.acc === "bigglasses" ? "#c92a2a" : "#2b2f3a";
      for (const dx of [-2.2, 2.2]) { c.beginPath(); c.arc(x + dx, ey, r, 0, 6.283); c.stroke(); }
      c.beginPath(); c.moveTo(x - 0.5, ey); c.lineTo(x + 0.5, ey); c.stroke();
    }
    this.headGear(p, x, y);
  }
  /** Headphones, headset, cap, hard hat and pencil: shared by people and animals. */
  private headGear(p: Person, x: number, y: number) {
    const c = this.ctx;
    if (p.acc === "phones" || p.acc === "headset") {
      c.lineWidth = 1; c.strokeStyle = "#343a40"; c.beginPath(); c.arc(x, y - 0.4, 6.8, Math.PI * 1.02, Math.PI * 1.98); c.stroke();
      const hc = p.acc === "phones" ? "#ff6b6b" : "#4c6ef5";
      this.box(x - 8, y - 1.4, 2.6, 4.4, 1.2, hc, 0.7); this.box(x + 5.4, y - 1.4, 2.6, 4.4, 1.2, hc, 0.7);
    }
    if (p.acc === "cap" || p.acc === "hardhat") {
      const col = p.acc === "hardhat" ? "#ffa94d" : p.capColor;
      c.beginPath(); c.arc(x, y - 0.8, 6.3, Math.PI, 0); c.closePath(); c.fillStyle = col; c.fill(); c.lineWidth = 0.9; c.strokeStyle = OUT; c.stroke();
      if (p.acc === "cap") this.box(x + 1, y - 1.7, 7, 1.7, 0.9, shade(col, -0.15), 0.8); else this.box(x - 7, y - 1.8, 14, 1.7, 0.8, shade(col, -0.1), 0.8);
    }
    if (p.acc === "pencil") { c.save(); c.translate(x + 5.6, y - 1.2); c.rotate(0.6); this.box(-0.6, -3.4, 1.3, 5.6, 0.5, "#ffd43b", 0.5); this.box(-0.6, -3.9, 1.3, 1, 0.4, "#ff8787", 0.4); c.restore(); }
  }
  private drawHeadAnimal(p: Person, x: number, y: number, t: number, seated: boolean) {
    const c = this.ctx, st = seated ? p.state : "idle", A = ANIMALS[ANIMAL_OF[p.id] ?? "mouse"], ey = y - 0.6;
    const tri = (pts: number[][], fill: string) => { c.beginPath(); c.moveTo(pts[0][0], pts[0][1]); pts.slice(1).forEach((q) => c.lineTo(q[0], q[1])); c.closePath(); c.fillStyle = fill; c.fill(); c.lineWidth = 0.9; c.strokeStyle = OUT; c.stroke(); };
    for (const s of [-1, 1]) {
      if (A.ear === "round") { this.ball(x + s * 4.8, y - 4.2, 2.6, 2.6, A.fur, 0.9); this.ball(x + s * 4.8, y - 4.2, 1.4, 1.4, A.inner, 0); }
      else if (A.ear === "point") { tri([[x + s * 5.8, y - 1], [x + s * 4.6, y - 8.6], [x + s * 1.2, y - 4.4]], A.fur); tri([[x + s * 4.8, y - 3], [x + s * 4.4, y - 6.6], [x + s * 2.6, y - 4.6]], A.inner); }
      else if (A.ear === "long") { this.ball(x + s * 2.8, y - 8.6, 1.9, 5, A.fur, 0.9, s * 0.12); this.ball(x + s * 2.8, y - 8.4, 0.9, 3.6, A.inner, 0, s * 0.12); }
      else if (A.ear === "floppy") this.ball(x + s * 6.2, y + 0.4, 2, 3.8, A.inner, 0.9, s * 0.2);
      else if (A.ear === "tuft") tri([[x + s * 5.6, y - 2], [x + s * 4.8, y - 7.6], [x + s * 1.6, y - 4.6]], A.fur);
    }
    if (A.mark === "frog") { for (const s of [-1, 1]) this.ball(x + s * 3.6, y - 4.4, 2.6, 2.5, A.fur, 0.9); }
    this.ball(x, y, 6.5, 5.6, A.fur, 1);
    if (A.mark === "panda") for (const s of [-1, 1]) this.ball(x + s * 2.6, ey, 1.9, 2.3, "#2b2f3a", 0, s * 0.4);
    if (A.mark === "tiger") { c.lineWidth = 0.9; c.strokeStyle = "#7a3b00"; c.beginPath(); c.moveTo(x, y - 5.4); c.lineTo(x, y - 3.4); c.moveTo(x - 2.4, y - 5); c.lineTo(x - 2, y - 3.4); c.moveTo(x + 2.4, y - 5); c.lineTo(x + 2, y - 3.4); c.moveTo(x - 6.2, y + 1); c.lineTo(x - 4.6, y + 1.4); c.moveTo(x + 6.2, y + 1); c.lineTo(x + 4.6, y + 1.4); c.stroke(); }
    if (A.mark === "fox") for (const s of [-1, 1]) this.ball(x + s * 3.4, y + 2.4, 3, 2.2, A.muz, 0);
    if (A.mark === "owl") { for (const s of [-1, 1]) this.ball(x + s * 2.7, y - 0.4, 2.9, 2.9, "#fff", 0.8); tri([[x - 1.2, y + 1.2], [x + 1.2, y + 1.2], [x, y + 3]], "#ffa94d"); }
    if (A.mark === "frog") { this.face(p, x, st, t, { dx: 3.6, ey: y - 4.4, my: y + 3, mw: 3 }); }
    else if (A.mark === "owl") { this.face(p, x, st, t, { dx: 2.7, ey: y - 0.4, my: y + 5.6, mw: 0.01 }); }
    else {
      this.ball(x, y + 2.6, 3.2, 2.3, A.muz, 0.6);
      this.ball(x, y + 1.7, A.mark === "koala" ? 1.7 : 0.95, A.mark === "koala" ? 1.3 : 0.7, A.nose ?? "#3b3150", 0);
      this.face(p, x, st, t, { dx: 2.5, ey, my: y + 4.2, mw: 1.2 });
    }
    if (p.acc === "phones" || p.acc === "headset" || p.acc === "cap" || p.acc === "hardhat") this.headGear(p, x, y);
  }
  /** Generic costume pieces (no real people or brands): wizard, knight, ninja, pirate and so on. */
  private drawCostume(p: Person, x: number, y: number) {
    const c = this.ctx, k = (this.style === "stars_in" ? STAR_IN[p.id] : this.style === "stars_world" ? STAR_WORLD[p.id] : HERO_OF[p.id]) ?? "courier";
    const poly = (pts: number[][], fill: string) => { c.beginPath(); c.moveTo(pts[0][0], pts[0][1]); pts.slice(1).forEach((q) => c.lineTo(q[0], q[1])); c.closePath(); c.fillStyle = fill; c.fill(); c.lineWidth = 0.9; c.strokeStyle = OUT; c.stroke(); };
    if (k === "wizard") { this.ball(x, y - 4, 8, 1.8, "#6741d9", 0.9); poly([[x - 5, y - 4.2], [x + 5, y - 4.2], [x + 1.5, y - 15], [x - 1, y - 14]], "#7950f2"); this.text("★", x + 0.4, y - 8.4, 3.4, "#ffd43b", "center"); }
    else if (k === "knight") { c.beginPath(); c.arc(x, y - 0.6, 6.6, Math.PI, 0); c.closePath(); c.fillStyle = "#ced4da"; c.fill(); c.lineWidth = 0.9; c.strokeStyle = OUT; c.stroke(); this.box(x - 5.2, y - 2.6, 10.4, 2.6, 1, "#adb5bd", 0.8); this.ball(x, y - 8, 1.2, 3.2, "#e03131", 0.7); }
    else if (k === "ninja") { this.box(x - 6.2, y - 3.2, 12.4, 2.8, 1.2, "#2b2f3a", 0.8); this.box(x - 5.6, y + 1.6, 11.2, 4.4, 2.4, "#2b2f3a", 0.8); poly([[x + 6, y - 2.4], [x + 11, y - 4], [x + 9.4, y - 0.8]], "#e03131"); }
    else if (k === "pirate") { c.beginPath(); c.arc(x, y - 0.8, 6.4, Math.PI, 0); c.closePath(); c.fillStyle = "#e03131"; c.fill(); c.lineWidth = 0.9; c.strokeStyle = OUT; c.stroke(); poly([[x + 5.6, y - 3], [x + 10, y - 1], [x + 6, y + 0.6]], "#e03131"); this.ball(x - 2.2, y + 0.9, 1.7, 1.5, "#222", 0.5); }
    else if (k === "astronaut") { this.ball(x, y, 8, 7.6, "rgba(170,215,255,.28)", 1.4, 0, "#f4f6fb"); this.box(x - 6, y + 5.6, 12, 2.4, 1.2, "#f4f6fb", 0.8); }
    else if (k === "detective") { this.ball(x, y - 3.6, 8.4, 1.8, "#8a5a2b", 0.9); this.box(x - 4.6, y - 9.2, 9.2, 6, 2.4, "#9b6a43", 0.9); this.box(x - 4.6, y - 5.4, 9.2, 1.4, 0.4, "#3b3150", 0.4); }
    else if (k === "firefighter") { c.beginPath(); c.arc(x, y - 0.8, 6.4, Math.PI, 0); c.closePath(); c.fillStyle = "#e03131"; c.fill(); c.lineWidth = 0.9; c.strokeStyle = OUT; c.stroke(); this.box(x - 7.4, y - 2, 14.8, 1.8, 0.8, "#c92a2a", 0.8); this.ball(x, y - 4.6, 1.5, 1.5, "#ffd43b", 0.6); }
    else if (k === "superhero") { poly([[x - 6.4, y - 1.4], [x - 3, y - 2.4], [x, y - 1.2], [x + 3, y - 2.4], [x + 6.4, y - 1.4], [x + 5.4, y + 1.8], [x + 2.2, y + 1.6], [x, y + 0.2], [x - 2.2, y + 1.6], [x - 5.4, y + 1.8]], "#1c7ed6"); for (const s of [-1, 1]) this.ball(x + s * 2.4, y + 0.1, 1, 0.8, "#fff", 0); }
    else if (k === "scholar") { poly([[x, y - 8.4], [x + 8, y - 5.6], [x, y - 3.2], [x - 8, y - 5.6]], "#2b2f3a"); this.box(x - 4, y - 4.8, 8, 2.6, 1, "#343a40", 0.7); this.ball(x + 6.6, y - 0.6, 0.9, 0.9, "#ffd43b", 0); }
    else if (k === "shades" || k === "aviator" || k === "divaglasses") {
      const col = k === "aviator" ? "#c9a227" : k === "divaglasses" ? "#e64980" : "#1b1b1b";
      for (const s of [-1, 1]) this.ball(x + s * 2.4, y + 0.7, 2.3, k === "aviator" ? 2.2 : 1.7, col, 0.8);
      c.lineWidth = 0.8; c.strokeStyle = OUT; c.beginPath(); c.moveTo(x - 0.4, y + 0.5); c.lineTo(x + 0.4, y + 0.5); c.moveTo(x - 4.7, y + 0.4); c.lineTo(x - 6, y); c.moveTo(x + 4.7, y + 0.4); c.lineTo(x + 6, y); c.stroke();
      this.ball(x - 3, y + 0.1, 0.6, 0.4, "rgba(255,255,255,.7)", 0);
      if (k === "divaglasses") this.text("★", x + 4.6, y - 2.2, 3.2, "#ffd43b", "center");
    }
    else if (k === "cricket") { c.beginPath(); c.arc(x, y - 0.8, 6.4, Math.PI, 0); c.closePath(); c.fillStyle = "#1c64d4"; c.fill(); c.lineWidth = 0.9; c.strokeStyle = OUT; c.stroke(); this.box(x + 1, y - 1.7, 7.4, 1.8, 0.9, "#154aa0", 0.8); this.ball(x, y - 4.2, 1.1, 1.1, "#fff", 0); }
    else if (k === "singer") { c.lineWidth = 1; c.strokeStyle = "#343a40"; c.beginPath(); c.arc(x, y - 0.4, 6.6, Math.PI * 1.02, Math.PI * 1.98); c.stroke(); this.box(x + 5.4, y - 1.2, 2.4, 4, 1.1, "#e64980", 0.7); c.lineWidth = 0.8; c.beginPath(); c.moveTo(x + 6.6, y + 2.8); c.quadraticCurveTo(x + 5.4, y + 6, x + 2.4, y + 5); c.stroke(); this.ball(x + 2.4, y + 5, 1, 1, "#343a40", 0); this.text("♪", x - 8, y - 6, 4.4, "#e64980", "center"); }
    else if (k === "director") { this.ball(x - 0.6, y - 4.6, 7, 2.4, "#d6336c", 0.9); this.ball(x - 0.4, y - 6.4, 2.4, 1.2, "#d6336c", 0.7); this.box(x + 5.4, y - 10, 5, 3.4, 0.8, "#343a40", 0.7); this.box(x + 5.4, y - 10, 5, 1, 0.4, "#fff", 0.5); }
    else if (k === "flowers") { [["#ff6b9d", -4.6], ["#ffd43b", -1.6], ["#ff922b", 1.6], ["#ff6b9d", 4.6]].forEach(([col, dx], i) => { this.ball(x + (dx as number), y - 4.6 + (i % 2 ? 0.6 : 0), 1.7, 1.7, col as string, 0.6); this.ball(x + (dx as number), y - 4.6 + (i % 2 ? 0.6 : 0), 0.6, 0.6, "#fff", 0); }); }
    else if (k === "tophat") { this.ball(x, y - 4.6, 7.4, 1.6, "#2b2f3a", 0.9); this.box(x - 4.4, y - 11.8, 8.8, 7.4, 1.4, "#343a40", 0.9); this.box(x - 4.4, y - 7.2, 8.8, 1.8, 0.4, "#e03131", 0.5); }
    else if (k === "headband") { this.box(x - 6.2, y - 3.2, 12.4, 2, 1, "#e03131", 0.8); poly([[x + 6, y - 2.4], [x + 9.4, y - 4], [x + 8.6, y - 0.6]], "#e03131"); }
    else if (k === "rock") { this.box(x - 6.4, y - 3.4, 12.8, 2.4, 1, "#2b2f3a", 0.8); for (let i = -2; i <= 2; i++) poly([[x + i * 2.4 - 1.2, y - 3.4], [x + i * 2.4, y - 8.4 + Math.abs(i) * 0.8], [x + i * 2.4 + 1.2, y - 3.4]], "#e64980"); for (const s of [-1, 1]) this.ball(x + s * 2.4, y + 0.7, 2.2, 1.6, "#1b1b1b", 0.8); }
    else if (k === "princess") { poly([[x - 4.6, y - 5.4], [x - 4.2, y - 9.4], [x - 2, y - 7], [x, y - 10.4], [x + 2, y - 7], [x + 4.2, y - 9.4], [x + 4.6, y - 5.4]], "#ffd43b"); this.ball(x, y - 7.2, 0.8, 0.8, "#fa5252", 0); }
    else { c.beginPath(); c.arc(x, y - 0.8, 6.3, Math.PI, 0); c.closePath(); c.fillStyle = p.capColor; c.fill(); c.lineWidth = 0.9; c.strokeStyle = OUT; c.stroke(); this.box(x + 1, y - 1.7, 7, 1.7, 0.9, shade(p.capColor, -0.15), 0.8); }
  }
  private drawCat(t: number) {
    const k = this.cat, x = k.x, y = k.y, c = this.ctx, step = k.moving ? Math.sin(t * 14) : 0;
    this.ball(x, y, 6, 1.5, "rgba(60,40,80,.2)", 0);
    if (k.sleep && !k.moving) {
      this.ball(x, y - 3, 6, 3.6, "#f4a259", 0.9); this.ball(x + 3.6, y - 3.4, 2.7, 2.5, "#f4a259", 0.9);
      this.ball(x - 2, y - 4.6, 1, 0.6, "#d9822b", 0, 0.5); this.ball(x + 1, y - 5.2, 1, 0.6, "#d9822b", 0, 0.1);
      c.lineWidth = 0.6; c.strokeStyle = OUT; c.beginPath(); c.arc(x + 4.4, y - 3.8, 0.7, 0.1, Math.PI - 0.1); c.stroke();
      const f = (t * 0.7) % 1; this.text("z", x + 6 + f * 3, y - 8 - f * 6, 3.6 + f * 1.4, `rgba(75,61,99,${(1 - f).toFixed(2)})`);
      return;
    }
    const sw = Math.sin(t * 4) * 2.2;
    c.lineWidth = 1.6; c.strokeStyle = OUT; c.beginPath(); c.moveTo(x - 5, y - 4.4); c.quadraticCurveTo(x - 9, y - 7 + sw * 0.3, x - 7 + sw * 0.4, y - 10); c.stroke();
    c.lineWidth = 0.9; c.strokeStyle = "#f4a259"; c.stroke();
    for (const dx of [-3.4, 3]) this.ball(x + dx, y - 1, 1.5, 1.6 + (dx > 0 ? step : -step) * 0.3, "#f4a259", 0.7);
    this.ball(x, y - 4.2, 5.8, 3.4, "#f4a259", 0.9);
    this.ball(x - 1, y - 6.2, 0.9, 0.5, "#d9822b", 0, 0.3); this.ball(x + 1.6, y - 6.4, 0.9, 0.5, "#d9822b", 0, -0.2);
    const hx = x + 5.2, hy = y - 7;
    c.beginPath(); c.moveTo(hx - 3, hy - 1); c.lineTo(hx - 2.4, hy - 5.2); c.lineTo(hx - 0.2, hy - 2.6); c.moveTo(hx + 0.4, hy - 2.8); c.lineTo(hx + 2.6, hy - 5.2); c.lineTo(hx + 3.2, hy - 0.6);
    c.fillStyle = "#f4a259"; c.fill(); c.lineWidth = 0.8; c.strokeStyle = OUT; c.stroke();
    this.ball(hx, hy, 3.6, 3.1, "#f4a259", 0.9);
    this.ball(hx - 1.3, hy - 0.4, 0.7, 0.9, "#222", 0); this.ball(hx + 1.3, hy - 0.4, 0.7, 0.9, "#222", 0);
    this.ball(hx, hy + 0.7, 0.5, 0.35, "#ff6b8a", 0);
  }
  private bubble(text: string, cx: number, y: number) {
    const c = this.ctx; c.font = `bold 4.4px ${FONT}`;
    const w = Math.ceil(c.measureText(text).width) + 7, x = Math.min(W - w - 2, Math.max(2, cx - w / 2));
    c.beginPath(); c.moveTo(cx - 2, y); c.lineTo(cx, y + 3.4); c.lineTo(cx + 2, y); c.fillStyle = "#fff"; c.fill(); c.lineWidth = 0.9; c.strokeStyle = OUT; c.stroke();
    this.box(x, y - 8.4, w, 9, 4.5, "#fff", 0.9);
    c.fillStyle = "#fff"; c.fillRect(cx - 1.6, y - 0.7, 3.2, 1.6);
    this.text(text, x + w / 2, y - 3.8, 4.4, "#2b2f3a", "center");
  }
  private chip(p: Person) {
    const c = this.ctx; c.font = `bold 4.3px ${FONT}`;
    const task = p.task ? (p.task.length > 15 ? p.task.slice(0, 14) + "…" : p.task) : "";
    const w = Math.max(c.measureText(p.label).width + 11, task ? c.measureText(task).width + 7 : 0) + 1;
    const x = p.desk[0] + 12 - w / 2, y = p.desk[1] + 14.5;
    this.box(x, y, w, task ? 12.5 : 8, 4, "rgba(255,255,255,.95)", 0.7);
    this.ball(x + 4, y + 4, 1.7, 1.7, STATUS_COLORS[p.state], 0.5);
    this.text(p.label, x + 7.5, y + 4, 4.3, "#2b2f3a");
    if (task) this.text(task, x + 3.5, y + 9.3, 3.8, "#5b5675");
  }
  private draw(t: number) {
    const c = this.ctx;
    c.setTransform(this.S, 0, 0, this.S, 0, 0);
    c.clearRect(0, 0, W, H);
    this.drawRooms(t);
    [...this.people].sort((a, b) => a.desk[1] - b.desk[1]).forEach((p) => this.drawDesk(p, t));
    [this.boy, this.cat as unknown as Person].sort((a, b) => a.y - b.y).forEach((e) => (e === (this.cat as unknown) ? this.drawCat(t) : this.drawPerson(e, t, false)));
    this.people.forEach((p) => this.chip(p));
    for (const f of this.flights) { const k = Math.min(1, f.t / f.dur), e = k * k * (3 - 2 * k); this.paper(f.fx + (f.tx - f.fx) * e, f.fy + (f.ty - f.fy) * e - Math.sin(k * Math.PI) * 8, k * 3); }
    for (const p of [...this.people, this.boy]) if (p.say) this.bubble(p.say, p.x, p.y - 24);
    if (this.selected && this.byId[this.selected]) {
      const p = this.byId[this.selected]; c.save(); c.setLineDash([2, 1.6]); c.lineWidth = 1; c.strokeStyle = "#4c6ef5";
      c.beginPath(); c.roundRect(p.x - 9, p.y - 25, 18, 28, 6); c.stroke(); c.restore();
    }
  }
}
