// Electron shell: starts the FastAPI backend and the Next.js UI as child processes, then opens a window on the UI.
// Data lives per user under %APPDATA%\<app>\data (AO_DATA_DIR). `--smoke` starts everything, checks both, and exits.
const { app, BrowserWindow, dialog, shell } = require("electron");
const { spawn, execFile } = require("child_process");
const http = require("http");
const net = require("net");
const path = require("path");
const fs = require("fs");

const ROOT = path.resolve(__dirname, "..");
const BACKEND_PORT = Number(process.env.AO_DESKTOP_BACKEND_PORT || 8765); // baked into the frontend build, see build-frontend.cmd
const UI_PORT = Number(process.env.AO_DESKTOP_UI_PORT || 3765);
const SMOKE = process.argv.includes("--smoke");
const children = [];

function portFree(port) {
  return new Promise((resolve) => {
    const s = net.createServer();
    s.once("error", () => resolve(false));
    s.once("listening", () => s.close(() => resolve(true)));
    s.listen(port, "127.0.0.1");
  });
}

function ping(url) {
  return new Promise((resolve) => {
    http.get(url, (r) => { r.resume(); resolve(r.statusCode < 500); }).on("error", () => resolve(false));
  });
}

async function waitFor(url, label, ms = 60000) {
  const end = Date.now() + ms;
  while (Date.now() < end) {
    if (await ping(url)) return;
    await new Promise((r) => setTimeout(r, 500));
  }
  throw new Error(`${label} did not start within ${ms / 1000}s`);
}

function start(cmd, args, opts, label) {
  const logDir = path.join(app.getPath("userData"), "logs");
  fs.mkdirSync(logDir, { recursive: true });
  const log = fs.openSync(path.join(logDir, `${label}.log`), "a");
  const p = spawn(cmd, args, { ...opts, stdio: ["ignore", log, log], windowsHide: true });
  p.on("error", (e) => fs.writeSync(log, `spawn error: ${e.message}\n`));
  children.push(p);
  return p;
}

function stopAll() {
  for (const p of children) {
    if (p.pid && !p.killed) {
      // /T kills the whole tree (uvicorn reloaders, next workers)
      execFile("taskkill", ["/pid", String(p.pid), "/T", "/F"], () => {});
    }
  }
  children.length = 0;
}

async function boot() {
  for (const [port, name] of [[BACKEND_PORT, "backend"], [UI_PORT, "UI"]]) {
    if (!(await portFree(port))) throw new Error(`Port ${port} (${name}) is already in use. Close the other program or set AO_DESKTOP_${name === "UI" ? "UI" : "BACKEND"}_PORT.`);
  }
  const dataDir = path.join(app.getPath("userData"), "data");
  fs.mkdirSync(dataDir, { recursive: true });
  const env = { ...process.env, AO_DATA_DIR: dataDir };
  if (app.isPackaged) {
    // installed build: the frozen backend and the standalone UI server ship inside the installer, nothing else to install
    const res = process.resourcesPath;
    start(path.join(res, "backend", "ao-backend.exe"), ["--port", String(BACKEND_PORT)], { cwd: path.join(res, "backend"), env }, "backend");
    start(process.execPath, [path.join(res, "ui", "server.js")],
      { cwd: path.join(res, "ui"), env: { ...process.env, ELECTRON_RUN_AS_NODE: "1", PORT: String(UI_PORT), HOSTNAME: "127.0.0.1" } }, "ui");
  } else {
    if (!fs.existsSync(path.join(ROOT, "frontend", ".next"))) {
      throw new Error("The UI is not built yet. Run build-frontend.cmd once, then start again.");
    }
    const python = process.env.AO_PYTHON || "python";
    start(python, ["-m", "uvicorn", "app.main:app", "--host", "127.0.0.1", "--port", String(BACKEND_PORT)],
      { cwd: path.join(ROOT, "backend"), env }, "backend");
    start(process.execPath, [path.join(ROOT, "frontend", "node_modules", "next", "dist", "bin", "next"), "start", "-p", String(UI_PORT), "-H", "127.0.0.1"],
      { cwd: path.join(ROOT, "frontend"), env: { ...process.env, ELECTRON_RUN_AS_NODE: "1" } }, "ui");
  }
  await waitFor(`http://127.0.0.1:${BACKEND_PORT}/docs`, "backend");
  await waitFor(`http://127.0.0.1:${UI_PORT}/`, "UI");
}

function openWindow() {
  const win = new BrowserWindow({ width: 1280, height: 860, title: "Agent Orchestrator", autoHideMenuBar: true });
  win.loadURL(`http://127.0.0.1:${UI_PORT}/`);
  win.webContents.setWindowOpenHandler(({ url }) => { shell.openExternal(url); return { action: "deny" }; });
}

const lock = app.requestSingleInstanceLock();
if (!lock) { app.quit(); }
app.on("window-all-closed", () => app.quit());
app.on("before-quit", stopAll);

app.whenReady().then(async () => {
  try {
    await boot();
    if (SMOKE) {
      // through the UI proxy too: proves the /api rewrite reaches the backend
      const viaUi = await ping(`http://127.0.0.1:${UI_PORT}/api/auth/status`);
      console.log(`SMOKE backend=ok ui=ok proxy=${viaUi ? "ok" : "FAILED"}`);
      stopAll();
      app.exit(viaUi ? 0 : 1);
      return;
    }
    openWindow();
  } catch (e) {
    stopAll();
    if (SMOKE) { console.error("SMOKE FAILED: " + e.message); app.exit(1); return; }
    dialog.showErrorBox("Agent Orchestrator could not start", e.message);
    app.exit(1);
  }
});
