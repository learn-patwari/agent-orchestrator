// Serves the exported UI (frontend/out) with fake /api answers, so the screens can be tested without the real backend.
import http from "node:http";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.join(path.dirname(fileURLToPath(import.meta.url)), "..", "..", "out");
const roles = ["orchestrator", "architect", "developer_1", "developer_2", "cicd", "tester", "incident_reporter", "production_support", "technical_writer"];
const states = ["idle", "done", "idle", "idle", "failed", "failed", "failed", "done", "failed"];
const team = roles.map((role, i) => ({ role, state: states[i], task: null, steps: 3, model: "sonnet", description: "role" }));
const types = { ".html": "text/html", ".js": "text/javascript", ".css": "text/css", ".json": "application/json", ".svg": "image/svg+xml", ".woff2": "font/woff2", ".txt": "text/plain" };
const api = {
  "/api/office": { team, handoffs: [], environments: { qc: null, prod: null } },
  "/api/budget": { daily: { used: 256800, limit: null, percent: null }, features: [] },
  "/api/status": team,
};

http
  .createServer((req, res) => {
    const url = new URL(req.url, "http://x").pathname;
    if (url.startsWith("/api/")) {
      res.writeHead(200, { "Content-Type": "application/json" });
      return res.end(JSON.stringify(api[url] ?? {}));
    }
    const rel = url.replace(/^\/+|\/+$/g, "");
    const file = [rel, rel + ".html", path.join(rel, "index.html")].map((f) => path.join(root, f)).find((f) => fs.existsSync(f) && fs.statSync(f).isFile());
    if (!file) {
      res.writeHead(404);
      return res.end("not found");
    }
    res.writeHead(200, { "Content-Type": types[path.extname(file)] ?? "application/octet-stream" });
    fs.createReadStream(file).pipe(res);
  })
  .listen(Number(process.env.PORT ?? 8797), "127.0.0.1");
