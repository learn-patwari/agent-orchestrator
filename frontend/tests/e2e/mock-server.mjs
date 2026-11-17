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
  "/api/features": [{ id: "f1", title: "notification feature", status: "srs_draft" }],
  "/api/features/f1": { id: "f1", title: "notification feature", status: "srs_draft", analysis: {}, qa: [], plan: null, srs_md: ["## 11. Sign-off", "| Item | Value |", "|---|---|", "| Approved by | |", "| Date | |"].join("\n") },
  "/api/features/f1/budget": { limit: null, used: 0, held: false },
  "/api/features/f1/skills": { available: [{ name: "postgres-tips", description: "How we use Postgres", tokens: 120 }], selected: [] },
  "/api/runs": [],
  "/api/pipeline/tasks": [],
  "/api/features/f1/setup": { repo_path: "", model: null },
  "/api/features/f1/graph": { built: false, reason: "not built yet; press Build" },
  "/api/chat": [],
  "/api/providers": [{ id: "p1", name: "Anthropic", type: "anthropic", base_url: "https://api.anthropic.com", api_key: "sk-...1234" }],
  "/api/providers/types": { types: ["openai", "anthropic", "claude_cli"] },
  "/api/providers/presets": { presets: [
    { id: "openai", name: "OpenAI", type: "openai", base_url: "https://api.openai.com/v1", needs_key: true, note: "" },
    { id: "openrouter", name: "OpenRouter", type: "openai", base_url: "https://openrouter.ai/api/v1", needs_key: true, note: "" },
  ] },
  "/api/providers/windows": [{ provider_id: "p1", name: "Anthropic", length_hours: 5, anchor: 0, ping: true, next_trigger: 1800000000, actual_reset: 1800003600, verdict: "differ" }],
  "/api/role-models": { roles: ["orchestrator"], models: {} },
  "/api/attention": { count: 3, items: [
    { id: "inbox:i1", kind: "approval", title: "Approve the SRS", detail: "", link: "/features/?id=f1", at: 3 },
    { id: "inbox:i2", kind: "question", title: "Which database?", detail: "", link: "/inbox/", at: 2 },
    { id: "run:r1", kind: "task_failed", title: "tester failed: Run tests", detail: "429", link: "/status/", at: 1 },
  ] },
  "/api/inbox": [{ id: "i1", kind: "approval", action: "srs_approval", feature_id: "f1", status: "open", title: "Approve the SRS" }],
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
