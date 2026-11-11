"""FastAPI entry point: auth, settings, providers, role models, price table."""
from contextlib import asynccontextmanager

import os
import threading
import time
from pathlib import Path

import httpx
from fastapi import Depends, FastAPI, Header, HTTPException, Response
from fastapi.responses import PlainTextResponse
from pydantic import BaseModel

from . import auth, db, secrets_store
from .ba import BAError
from .agent_registry import AgentRegistry, RegistryError, key_ok, registry_key
from .budget import Budget
from .plugins import Plugins
from .mcp import McpError, McpServers, McpTools, connect as mcp_connect
from .profiles import BUILTIN as BUILTIN_PROFILES, ProfileError, Profiles
from .features import Features, NotApproved, features as features_repo, require_approved
from .inbox import Conflict, Inbox
from .notify import Notifier
from .pipeline.flow import Flow, FlowError, tasks as tasks_repo
from .pipeline.github import GhClient
from .pipeline.merge_exec import MergeRefused, merge_batch
from .pipeline.merge_gate import GateConfig
from .incidents import IncidentError, IncidentTool, Incidents
from .pipeline.agent_tool import PipelineTool
from .pipeline.docs_gate import DocsGate, state as docs_state
from .pipeline.release import Release
from .pipeline.worktrees import WorktreeError, WorktreeManager
from .orchestrator import DelegationError, Orchestrator, check_workspace, default_root, status_table, team_check, team_snapshot
from .scheduler import Scheduler, budget_percent_used
from .providers import pricing, registry
from .providers.claude_cli import normalise_exe
from .providers.presets import PRESETS
from .providers.base import ProviderError
from .runtime import runner as rt
from .runtime.roles import DEVELOPERS, load_roles, validate
from .runtime.skills import SkillRegistry
from .runtime.tools import ToolBox, Workspace

providers_repo = db.DocRepo("providers")
ROLES = ["orchestrator", "architect", "developer_1", "developer_2", "tester", "cicd",
         "incident_reporter", "production_support", "technical_writer"]
handoffs_repo = db.DocRepo("handoffs")


JOB_KINDS = {"reminder"}  # more kinds (start a run, resume a lane) arrive with the pipeline milestones


def run_job(job: dict) -> None:
    if job["payload"]["kind"] == "reminder":
        Inbox().post("status", job["name"], job["payload"].get("text", ""))
        Notifier().send("reminder", job["name"], job["payload"].get("text", ""), key=f"job:{job['id']}")


def scheduler_loop(stop: threading.Event, every: float = 30.0, restart_held=None, restart_daily=None) -> None:
    """Local background loop: schedules run only while the app is running."""
    import time as _t
    while not stop.wait(every):
        try:
            st = registry.get_settings()
            Scheduler(st["pause_at_percent"]).tick(_t.time(), budget_percent_used(st["daily_budget_usd"]), run_job)
        except Exception:
            pass  # never let the loop die; failures of single jobs are recorded by tick()
        try:  # features held at their token limit continue once the time the person set has come
            for fid in Budget().tick(_t.time()):
                if restart_held:
                    restart_held(fid)
            if restart_daily:
                restart_daily()
        except Exception:
            pass


@asynccontextmanager
async def lifespan(app: FastAPI):
    db.init_db()
    stop = threading.Event()
    if not os.environ.get("AO_NO_SCHEDULER"):
        threading.Thread(target=scheduler_loop, args=(stop, 30.0, getattr(app.state, "restart_held", None),
                                           getattr(app.state, "restart_daily", None)),
                         daemon=True).start()
    yield
    stop.set()


def create_app(http_client=None, gh_factory=None) -> FastAPI:
    app = FastAPI(title="Agent Orchestrator", lifespan=lifespan)
    app.state.http_client = http_client  # tests inject a mock transport client
    guard = [Depends(auth.require_auth)]

    class Password(BaseModel):
        password: str

    @app.get("/api/health")
    def health():
        return {"ok": True}

    @app.get("/api/auth/status")
    def auth_status():
        return {"setup_done": auth.has_user()}

    def _login_response(resp: Response) -> dict:
        resp.set_cookie("ao_session", auth.make_token(), httponly=True, samesite="strict",
                        max_age=auth.SESSION_TTL)
        return {"ok": True}

    @app.post("/api/auth/setup")
    def setup(body: Password, resp: Response):
        auth.set_password(body.password)
        return _login_response(resp)

    @app.post("/api/auth/login")
    def login(body: Password, resp: Response):
        if not auth.verify_password(body.password):
            raise HTTPException(401, "invalid credentials")
        return _login_response(resp)

    @app.post("/api/auth/logout")
    def logout(resp: Response):
        resp.delete_cookie("ao_session")
        return {"ok": True}

    # ---- settings (FR-1, feature flags) ----
    @app.get("/api/settings", dependencies=guard)
    def get_settings():
        return registry.get_settings()

    @app.put("/api/settings", dependencies=guard)
    def put_settings(patch: dict):
        cur = db.kv_get("settings", {})
        unknown = set(patch) - set(registry.get_settings())
        if unknown:
            raise HTTPException(400, f"unknown settings: {sorted(unknown)}")
        if "feature_flags" in patch:
            patch["feature_flags"] = {**cur.get("feature_flags", {}), **patch["feature_flags"]}
        db.kv_set("settings", {**cur, **patch})
        return registry.get_settings()

    # ---- providers (FR-6..FR-9) ----
    class ProviderIn(BaseModel):
        name: str
        type: str
        base_url: str = ""
        api_key: str = ""

    def _public(rec: dict) -> dict:
        out = {k: v for k, v in rec.items() if k != "api_key_enc"}
        out["api_key"] = secrets_store.mask(secrets_store.decrypt(rec["api_key_enc"]))
        return out

    @app.get("/api/providers/presets", dependencies=guard)
    def provider_presets():
        return {"presets": PRESETS}

    @app.get("/api/providers/types", dependencies=guard)
    def provider_types():
        return {"types": registry.allowed_types()}

    @app.get("/api/providers", dependencies=guard)
    def list_providers():
        return [_public(p) for p in providers_repo.list()]

    @app.post("/api/providers", dependencies=guard)
    def add_provider(body: ProviderIn):
        if body.type not in registry.allowed_types():
            raise HTTPException(400, f"provider type not available: {body.type}")
        rec = providers_repo.create({
            "name": body.name, "type": body.type, "base_url": (normalise_exe(body.base_url) if body.type == "claude_cli" else body.base_url.rstrip("/")),
            "api_key_enc": secrets_store.encrypt(body.api_key),
        })
        return _public(rec)

    @app.delete("/api/providers/{pid}", dependencies=guard)
    def delete_provider(pid: str):
        if not providers_repo.delete(pid):
            raise HTTPException(404, "not found")
        return {"ok": True}

    class TestIn(BaseModel):
        model: str

    @app.post("/api/providers/{pid}/test", dependencies=guard)
    def test_provider(pid: str, body: TestIn):
        rec = providers_repo.get(pid)
        if not rec:
            raise HTTPException(404, "not found")
        try:
            adapter = registry.build(rec, client=app.state.http_client)
            return adapter.test_connection(body.model)
        except ProviderError as e:
            return {"ok": False, "error_kind": e.kind, "error": secrets_store.redact(str(e))}
        except ValueError as e:
            raise HTTPException(400, str(e))
        except Exception as e:  # never let an adapter crash become a silent 500 in the UI
            return {"ok": False, "error_kind": "unknown", "error": secrets_store.redact(f"{type(e).__name__}: {e}")}

    # ---- per-role models (FR-8) ----
    def _role_names() -> list[str]:
        """Built-in roles plus any role file the user added, so every role can be given a model."""
        return list(dict.fromkeys([*ROLES, *load_roles(registry.get_settings()["role_dirs"])]))

    @app.get("/api/role-models", dependencies=guard)
    def get_role_models():
        return {"roles": _role_names(), "models": db.kv_get("role_models", {})}

    @app.put("/api/role-models/{role}", dependencies=guard)
    def set_role_model(role: str, body: dict):
        if role not in _role_names():
            raise HTTPException(404, "unknown role")
        if not providers_repo.get(body.get("provider_id", "")) or not body.get("model"):
            raise HTTPException(400, "need a known provider_id and a model")
        m = db.kv_get("role_models", {})
        m[role] = {"provider_id": body["provider_id"], "model": body["model"]}
        db.kv_set("role_models", m)
        return m

    # ---- prices (A-18) ----
    @app.get("/api/prices", dependencies=guard)
    def get_prices():
        return pricing.get_table()

    @app.put("/api/prices", dependencies=guard)
    def put_prices(table: dict):
        try:
            pricing.set_table(table)
        except ValueError as e:
            raise HTTPException(400, str(e))
        return pricing.get_table()

    @app.get("/api/usage", dependencies=guard)
    def usage():
        calls = db.DocRepo("llm_calls").list()
        by_role: dict = {}
        for c in calls:
            r = by_role.setdefault(c["role"], {"calls": 0, "input_tokens": 0, "output_tokens": 0, "cost_usd": 0.0})
            r["calls"] += 1
            r["input_tokens"] += c["input_tokens"]
            r["output_tokens"] += c["output_tokens"]
            r["cost_usd"] += c["cost_usd"] or 0.0
        return {"by_role": by_role}

    class ScanIn(BaseModel):
        text: str

    @app.post("/api/secrets/scan", dependencies=guard)
    def scan(body: ScanIn):
        """FR-4: chat UI calls this before sending a message to warn about pasted credentials."""
        return {"looks_like_secret": secrets_store.looks_like_secret(body.text)}

    # ---- skills, roles, runs (M3) ----
    def _registry() -> SkillRegistry:
        return SkillRegistry(registry.get_settings()["skill_paths"])

    @app.get("/api/skills", dependencies=guard)
    def list_skills():
        return _registry().catalog()

    @app.get("/api/roles", dependencies=guard)
    def list_roles():
        known_skills = set(_registry().scan())
        known_tools = set(ToolBox(Workspace(Path(".")), None).specs)
        return [{"name": r.name, "description": r.description, "tools": r.tools, "skills": r.skills,
                 "model": r.model, "source": r.source, "problems": validate(r, known_tools, known_skills)}
                for r in load_roles(registry.get_settings()["role_dirs"]).values()]

    class RunIn(BaseModel):
        role: str
        task: str
        workspace: str
        wait: bool = False
        feature_id: str | None = None
        task_id: str | None = None

    class LaunchError(Exception):
        def __init__(self, status: int, msg: str):
            super().__init__(msg)
            self.status = status

    def _agent_flow() -> Flow:
        # agents report steps on existing tasks; creating worktrees stays a human/API action
        st = registry.get_settings()
        return Flow(None, st["integration_branch"], st["review_round_limit"])

    def _launch(role_name: str, task: str, workspace: str, wait: bool = False, feature_id: str | None = None,
                task_id: str | None = None):
        """Start one agent run. Shared by POST /api/runs and the Orchestrator's delegate tool."""
        if role_name in DEVELOPERS:  # FR-33: no development before the human approved the SRS
            try:
                require_approved(feature_id)
            except NotApproved as e:
                raise LaunchError(409, str(e))
        roles = load_roles(registry.get_settings()["role_dirs"])
        role = roles.get(role_name)
        if not role:
            raise LaunchError(404, f"unknown role {role_name}")
        st = registry.get_settings()
        try:
            ws = check_workspace(workspace, st["workspace_roots"] or [str(default_root())])
        except ValueError as e:
            raise LaunchError(400, str(e))
        pct = budget_percent_used(st["daily_budget_usd"])
        if pct is not None and pct >= 100:
            Notifier().send("budget_pause", "Daily LLM budget reached",
                            f"Used {pct:.0f}% of the daily cap; new runs are blocked.", key="budget_pause")
            raise LaunchError(429, "daily LLM budget reached; new runs are blocked")
        why = Budget().window_gate(time.time()) or Budget().daily_gate(time.time(), st["daily_token_limit"])
        if why:
            raise LaunchError(429, why)
        held = Budget().gate(feature_id)
        if held:
            raise LaunchError(429, f"this feature is {held}; press Resume once your window has cleared")
        runner = _make_runner(role, ws, feature_id, task_id)
        run = runner.start(task, str(ws), feature_id, task_id)
        if wait:
            return runner.run(run["id"])
        threading.Thread(target=runner.run, args=(run["id"],), daemon=True).start()
        return run

    def _skill_names(feature_id: str | None, task_id: str | None) -> list[str]:
        """Skills preloaded for a run: the feature's, then the task's, without repeats."""
        names = (features_repo.get(feature_id) or {}).get("skills", []) if feature_id else []
        names = names + ((tasks_repo.get(task_id) or {}).get("skills", []) if task_id else [])
        return list(dict.fromkeys(names))

    def _make_runner(role, ws, feature_id: str | None = None, task_id: str | None = None) -> rt.Runner:
        mapping = db.kv_get("role_models", {}).get(role.name)
        if not mapping:
            raise LaunchError(400, f"no model configured for role {role.name}")
        rec = providers_repo.get(mapping["provider_id"])
        if not rec:
            raise LaunchError(400, "provider for this role no longer exists")
        try:
            provider = registry.build(rec, client=app.state.http_client)
        except ValueError as e:
            raise LaunchError(400, str(e))
        reg = _registry()
        return rt.Runner(provider, mapping["model"], role,
                         ToolBox(Workspace(ws), reg, pipeline=PipelineTool(role.name, _agent_flow),
                                 incidents=IncidentTool(role.name), mcp=McpTools(role.name)), reg,
                         Profiles().limits(),
                         provider_id=rec["id"], gate=lambda: (Budget().window_gate(time.time())
                                       or Budget().daily_gate(time.time(), registry.get_settings()["daily_token_limit"])
                                       or (Budget().gate(feature_id) if feature_id else None)),
                         preload=reg.estimate(_skill_names(feature_id, task_id))["bodies"])

    def _restart_runs(match) -> list[str]:
        """Held runs that match continue from where they stopped."""
        roles = load_roles(registry.get_settings()["role_dirs"])
        started = []
        for r in rt.runs.list():
            if r["status"] != "held" or r["role"] not in roles or not match(r):
                continue
            try:
                runner = _make_runner(roles[r["role"]], r["workspace"], r.get("feature_id"), r.get("task_id"))
            except LaunchError:
                continue  # stays held; the person sees it on the runs page
            rt.runs.update(r["id"], {"status": "running", "error": None, "finished_at": None})
            if os.environ.get("AO_SYNC_RUNS") == "1":
                runner.run(r["id"])
            else:
                threading.Thread(target=runner.run, args=(r["id"],), daemon=True).start()
            started.append(r["id"])
        return started

    def _restart_held(fid: str) -> list[str]:
        """After a resume: runs held for this feature continue."""
        return _restart_runs(lambda r: r.get("feature_id") == fid)

    def _restart_daily() -> list[str]:
        """Runs held by the daily limit or the usage window continue once that guard has room again."""
        now, b = time.time(), Budget()
        daily_ok = not b.daily_gate(now, registry.get_settings()["daily_token_limit"])
        window_ok = not b.window_gate(now)

        def due(r):
            e = str(r.get("error", ""))
            return (e.startswith("daily token limit") and daily_ok and window_ok) or \
                   (e.startswith("usage window") and window_ok and daily_ok)
        return _restart_runs(due)

    app.state.restart_daily = _restart_daily
    app.state.restart_held = _restart_held

    # ---- plugins (FR-20 to FR-22) ----
    def _known() -> tuple[set, set, set]:
        roles = set(load_roles(registry.get_settings()["role_dirs"]))
        tools = set(ToolBox(Workspace(Path(".")), None).specs)
        return roles, tools, set(_registry().scan())

    class PluginIn(BaseModel):
        path: str

    @app.get("/api/plugins", dependencies=guard)
    def plugins_list():
        return {"installed": Plugins().list(), "available": Plugins().available()}

    @app.post("/api/plugins/install", dependencies=guard)
    def plugins_install(body: PluginIn):
        try:
            return Plugins().install(body.path, *_known())
        except ValueError as e:
            raise HTTPException(400, str(e))

    @app.post("/api/agents/build", dependencies=guard)
    def agents_build(body: dict):
        try:
            return Plugins().build_agent(body, *_known())
        except ValueError as e:
            raise HTTPException(400, str(e))

    @app.get("/api/plugins/{name}/export", dependencies=guard)
    def plugins_export(name: str):
        try:
            data = Plugins().export_zip(name)
        except KeyError:
            raise HTTPException(404, "not found")
        return Response(data, media_type="application/zip",
                        headers={"Content-Disposition": f'attachment; filename="{name}.zip"'})

    @app.post("/api/plugins/{name}/enable", dependencies=guard)
    def plugins_enable(name: str):
        try:
            return Plugins().enable(name, *_known())
        except KeyError:
            raise HTTPException(404, "not found")
        except ValueError as e:
            raise HTTPException(409, str(e))

    @app.post("/api/plugins/{name}/disable", dependencies=guard)
    def plugins_disable(name: str):
        try:
            return Plugins().disable(name)
        except KeyError:
            raise HTTPException(404, "not found")

    @app.delete("/api/plugins/{name}", dependencies=guard)
    def plugins_remove(name: str):
        if not Plugins().remove(name):
            raise HTTPException(404, "not found")
        return {"ok": True}

    # ---- MCP servers (FR-44) ----
    @app.get("/api/mcp/servers", dependencies=guard)
    def mcp_list():
        return McpServers().list()

    @app.put("/api/mcp/servers/{name}", dependencies=guard)
    def mcp_save(name: str, body: dict):
        try:
            return McpServers().save(name, body)
        except McpError as e:
            raise HTTPException(400, str(e))

    @app.delete("/api/mcp/servers/{name}", dependencies=guard)
    def mcp_delete(name: str):
        if not McpServers().delete(name):
            raise HTTPException(404, "not found")
        return {"ok": True}

    @app.post("/api/mcp/servers/{name}/test", dependencies=guard)
    def mcp_test(name: str):
        server = McpServers().get(name)
        if server is None:
            raise HTTPException(404, "not found")
        try:
            c = mcp_connect(server, app.state.http_client)
        except McpError as e:
            return {"ok": False, "error": secrets_store.redact(str(e)), "tools": []}
        try:
            tools = [{"name": t["name"], "description": t.get("description", "")} for t in c.list_tools()]
            return {"ok": True, "error": None, "tools": tools}
        except McpError as e:
            return {"ok": False, "error": secrets_store.redact(str(e)), "tools": []}
        finally:
            c.close()

    @app.get("/api/budget", dependencies=guard)
    def budget_overview():
        return Budget().overview(time.time(), registry.get_settings()["daily_token_limit"])

    @app.get("/api/features/{fid}/budget", dependencies=guard)
    def get_budget(fid: str):
        _feature_or_404(fid)
        return Budget().status(fid)

    class BudgetIn(BaseModel):
        limit: int | None = None
        resume_at: float | None = None

    @app.put("/api/features/{fid}/budget", dependencies=guard)
    def set_budget(fid: str, body: BudgetIn):
        _feature_or_404(fid)
        try:
            Budget().set(fid, body.limit, body.resume_at)
        except ValueError as e:
            raise HTTPException(400, str(e))
        return Budget().status(fid)

    def _profiles_view() -> dict:
        p = Profiles()
        return {"active": p.active()["name"], "profiles": p.list()}

    @app.get("/api/profiles", dependencies=guard)
    def get_profiles():
        return _profiles_view()

    @app.put("/api/profiles/{name}", dependencies=guard)
    def save_profile(name: str, body: dict):
        try:
            Profiles().save(name, body)
        except ProfileError as e:
            raise HTTPException(400, str(e))
        return _profiles_view()

    @app.delete("/api/profiles/{name}", dependencies=guard)
    def delete_profile(name: str):
        try:
            Profiles().delete(name)
        except ProfileError as e:
            raise HTTPException(400 if name in BUILTIN_PROFILES else 404, str(e))
        return _profiles_view()

    class ActiveIn(BaseModel):
        name: str

    @app.post("/api/profiles/active", dependencies=guard)
    def set_active_profile(body: ActiveIn):
        try:
            Profiles().set_active(body.name)
        except ProfileError as e:
            raise HTTPException(404, str(e))
        return _profiles_view()

    def _skills_view(fid: str) -> dict:
        reg = _registry()
        selected = features_repo.get(fid).get("skills", [])
        available = []
        for s in reg.catalog():
            available.append({**s, "tokens": reg.estimate([s["name"]])["tokens"]})
        return {"selected": selected, "tokens": reg.estimate(selected)["tokens"], "available": available}

    class SkillsIn(BaseModel):
        skills: list[str]

    @app.put("/api/pipeline/tasks/{tid}/skills", dependencies=guard)
    def set_task_skills(tid: str, body: SkillsIn):
        t = tasks_repo.get(tid)
        if t is None:
            raise HTTPException(404, "not found")
        names = list(dict.fromkeys(body.skills))
        unknown = _registry().estimate(names)["unknown"]
        if unknown:
            raise HTTPException(400, f"unknown skill(s): {', '.join(unknown)}")
        tasks_repo.update(tid, {"skills": names})
        return {"selected": names, "tokens": _registry().estimate(names)["tokens"]}

    @app.get("/api/features/{fid}/skills", dependencies=guard)
    def get_feature_skills(fid: str):
        _feature_or_404(fid)
        return _skills_view(fid)

    @app.put("/api/features/{fid}/skills", dependencies=guard)
    def set_feature_skills(fid: str, body: SkillsIn):
        _feature_or_404(fid)
        names = list(dict.fromkeys(body.skills))
        unknown = _registry().estimate(names)["unknown"]
        if unknown:
            raise HTTPException(400, f"unknown skill(s): {', '.join(unknown)}")
        features_repo.update(fid, {"skills": names})
        return _skills_view(fid)

    @app.post("/api/features/{fid}/resume", dependencies=guard)
    def resume_feature(fid: str):
        _feature_or_404(fid)
        status = Budget().resume(fid)
        _restart_held(fid)
        return status

    @app.post("/api/runs", dependencies=guard)
    def start_run(body: RunIn):
        try:
            return _launch(body.role, body.task, body.workspace, body.wait, body.feature_id, body.task_id)
        except LaunchError as e:
            raise HTTPException(e.status, str(e))

    @app.get("/api/runs", dependencies=guard)
    def list_runs():
        return rt.runs.list()

    @app.get("/api/runs/{rid}", dependencies=guard)
    def get_run(rid: str):
        run = rt.runs.get(rid)
        if not run:
            raise HTTPException(404, "not found")
        return {**run, "log": rt.load_log(rid)}

    # ---- inbox, assumptions, audit (M4) ----
    def _inbox() -> Inbox:
        return Inbox(decide_with_defaults=registry.get_settings()["decide_with_defaults"])

    def _inbox_call(fn, *a):
        try:
            return fn(*a)
        except KeyError:
            raise HTTPException(404, "not found")
        except Conflict as e:
            raise HTTPException(409, str(e))
        except ValueError as e:
            raise HTTPException(400, str(e))

    class AnswerIn(BaseModel):
        answer: str

    class DecideIn(BaseModel):
        approve: bool

    class OverruleIn(BaseModel):
        value: str

    @app.get("/api/inbox", dependencies=guard)
    def list_inbox(status: str | None = None):
        return Inbox().list(status)

    def _synced(item: dict) -> dict:
        Features(None, "", decide_with_defaults=registry.get_settings()["decide_with_defaults"]).sync(item)
        return item

    @app.post("/api/inbox/{iid}/answer", dependencies=guard)
    def answer_item(iid: str, body: AnswerIn):
        return _synced(_inbox_call(Inbox().answer, iid, body.answer))

    @app.post("/api/inbox/{iid}/accept-default", dependencies=guard)
    def accept_default(iid: str):
        return _synced(_inbox_call(Inbox().accept_default, iid))

    @app.post("/api/inbox/{iid}/decide", dependencies=guard)
    def decide(iid: str, body: DecideIn):
        return _synced(_inbox_call(Inbox().decide, iid, body.approve))

    @app.get("/api/assumptions", dependencies=guard)
    def list_assumptions():
        return Inbox().assumptions()

    @app.post("/api/assumptions/{aid}/overrule", dependencies=guard)
    def overrule(aid: str, body: OverruleIn):
        return _inbox_call(Inbox().overrule, aid, body.value)

    @app.get("/api/audit", dependencies=guard)
    def audit():
        return db.DocRepo("audit").list()

    # ---- dev/test/review pipeline and merge queue (M6) ----
    def _flow() -> Flow:
        st = registry.get_settings()
        if not st["repo_path"]:
            raise HTTPException(400, "set repo_path in settings first")
        try:
            from .config import data_dir
            root = st["worktree_root"] or str(data_dir() / "worktrees")
            return Flow(WorktreeManager(Path(st["repo_path"]), Path(root)), st["integration_branch"],
                        st["review_round_limit"])
        except WorktreeError as e:
            raise HTTPException(400, str(e))

    def _flow_call(fn, *a, **kw):
        try:
            return fn(*a, **kw)
        except (FlowError, WorktreeError) as e:
            raise HTTPException(409, str(e))

    class TaskIn(BaseModel):
        feature_id: str
        name: str
        stacked_on: str | None = None
        owner: str = "developer_1"

    class ArchitectIn(BaseModel):
        approve: bool

    class QcIn(BaseModel):
        passed: int
        failed: int

    class ProdApproveIn(BaseModel):
        by: str

    class DevDoneIn(BaseModel):
        pr_number: int
        head_sha: str

    class TestsIn(BaseModel):
        passed: int
        failed: int
        area: str

    class ReviewIn(BaseModel):
        body: str

    class MergeIn(BaseModel):
        numbers: list[int]

    @app.get("/api/pipeline/tasks", dependencies=guard)
    def list_tasks():
        return tasks_repo.list()

    @app.post("/api/pipeline/tasks", dependencies=guard)
    def start_task(body: TaskIn):
        return _flow_call(_flow().start, body.feature_id, body.name, body.stacked_on, body.owner)

    @app.post("/api/pipeline/tasks/{tid}/dev-done", dependencies=guard)
    def task_dev_done(tid: str, body: DevDoneIn):
        return _flow_call(_flow().dev_done, tid, body.pr_number, body.head_sha)

    @app.post("/api/pipeline/tasks/{tid}/tests", dependencies=guard)
    def task_tests(tid: str, body: TestsIn):
        return _flow_call(_flow().record_tests, tid, body.passed, body.failed, body.area)

    @app.post("/api/pipeline/tasks/{tid}/review", dependencies=guard)
    def task_review(tid: str, body: ReviewIn):
        return _flow_call(_flow().record_review, tid, body.body)

    # release stages after peer review: architect, QC, the human's production gate
    @app.post("/api/pipeline/tasks/{tid}/architect", dependencies=guard)
    def task_architect(tid: str, body: ArchitectIn):
        return _flow_call(Release().architect_review, tid, body.approve)

    @app.post("/api/pipeline/tasks/{tid}/deploy-qc", dependencies=guard)
    def task_deploy_qc(tid: str):
        return _flow_call(Release().deploy_qc, tid)

    @app.post("/api/pipeline/tasks/{tid}/qc", dependencies=guard)
    def task_qc(tid: str, body: QcIn):
        return _flow_call(Release().qc_result, tid, body.passed, body.failed)

    class DocsCheckIn(BaseModel):
        version: str
        endpoints: list[str] = []
        flagged: list[dict] = []
        gaps: list[dict] = []

    class DocsExceptionIn(BaseModel):
        reason: str
        by: str = "human"

    @app.post("/api/pipeline/tasks/{tid}/docs-check", dependencies=guard)
    def task_docs_check(tid: str, body: DocsCheckIn):
        if tasks_repo.get(tid) is None:
            raise HTTPException(404, "unknown task")
        return _flow_call(DocsGate().run, tid, body.version, body.endpoints, body.flagged, body.gaps)

    @app.post("/api/pipeline/tasks/{tid}/docs-exception", dependencies=guard)
    def task_docs_exception(tid: str, body: DocsExceptionIn):
        if tasks_repo.get(tid) is None:
            raise HTTPException(404, "unknown task")
        if body.by != "human":
            raise HTTPException(403, "only the human can ship with a documented exception")
        return _flow_call(DocsGate().exception, tid, body.reason, body.by)

    class WindowIn(BaseModel):
        percent: float
        resets_at: float | None = None

    @app.get("/api/usage-window", dependencies=guard)
    def get_usage_window():
        return Budget().window(time.time())

    @app.put("/api/usage-window", dependencies=guard)
    def put_usage_window(body: WindowIn):
        if not 0 <= body.percent <= 100:
            raise HTTPException(400, "percent must be between 0 and 100")
        return Budget().set_window(body.percent, body.resets_at)

    @app.post("/api/pipeline/tasks/{tid}/request-prod", dependencies=guard)
    def task_request_prod(tid: str):
        return _flow_call(Release().request_prod, tid)

    @app.post("/api/pipeline/tasks/{tid}/approve-prod", dependencies=guard)
    def task_approve_prod(tid: str, body: ProdApproveIn):
        return _flow_call(Release().approve_prod, tid, body.by)

    @app.post("/api/pipeline/tasks/{tid}/deploy-prod", dependencies=guard)
    def task_deploy_prod(tid: str):
        return _flow_call(Release().deployed_prod, tid)

    class ProdTestsIn(BaseModel):
        task_id: str
        passed: int
        failed: int
        area: str

    @app.post("/api/pipeline/prod-tests", dependencies=guard)
    def prod_tests(body: ProdTestsIn):
        """A production test report. A failing one raises an incident for production support, once."""
        try:
            return {"incident": Incidents().production_tests(body.task_id, body.passed, body.failed, body.area)}
        except IncidentError as e:
            raise HTTPException(404, str(e))

    @app.get("/api/incidents", dependencies=guard)
    def list_incidents():
        return Incidents().list()

    @app.post("/api/pipeline/merge", dependencies=guard)
    def merge(body: MergeIn):
        st = registry.get_settings()
        flow = _flow()
        factory = gh_factory or (lambda repo, lookup: GhClient(repo, tests_lookup=lookup))
        gh = factory(st["github_repo"], flow.tests_for_pr)
        cfg = GateConfig(integration_branch=st["integration_branch"], review_round_limit=st["review_round_limit"])
        try:
            out = merge_batch(gh, body.numbers, cfg, st["merge_style"], st["reviewer_can_merge"])
        except MergeRefused as e:
            raise HTTPException(403, str(e))
        for m in out["merged"]:
            flow.mark_merged(m["number"])
        return out

    # ---- feature intake (M5) ----
    class FeatureIn(BaseModel):
        title: str
        text: str

    def _architect() -> Features:
        mapping = db.kv_get("role_models", {}).get("architect")
        rec = providers_repo.get(mapping["provider_id"]) if mapping else None
        if not rec:
            raise HTTPException(400, "no model configured for the architect role")
        return Features(registry.build(rec, client=app.state.http_client), mapping["model"], rec["id"],
                        registry.get_settings()["decide_with_defaults"])

    def _feature_or_404(fid: str) -> dict:
        f = features_repo.get(fid)
        if not f:
            raise HTTPException(404, "not found")
        return f

    @app.post("/api/features", dependencies=guard)
    def create_feature(body: FeatureIn):
        ft = _architect()
        try:
            return ft.intake(body.title, body.text)
        except (BAError, ProviderError) as e:
            raise HTTPException(502, secrets_store.redact(f"architect failed: {e}"))

    @app.get("/api/features", dependencies=guard)
    def list_features():
        return [{k: v for k, v in f.items() if k not in ("srs_md",)} for f in features_repo.list()]

    @app.get("/api/features/{fid}", dependencies=guard)
    def get_feature(fid: str):
        return _feature_or_404(fid)

    @app.get("/api/features/{fid}/srs", dependencies=guard)
    def get_srs(fid: str):
        f = _feature_or_404(fid)
        if not f.get("srs_md"):
            raise HTTPException(404, "no SRS yet")
        return PlainTextResponse(f["srs_md"])

    @app.post("/api/features/{fid}/finalize", dependencies=guard)
    def finalize_feature(fid: str):
        _feature_or_404(fid)
        ft = _architect()
        try:
            return ft.finalize(fid)
        except ValueError as e:
            raise HTTPException(409, str(e))
        except (BAError, ProviderError) as e:
            raise HTTPException(502, secrets_store.redact(f"architect failed: {e}"))

    # ---- orchestrator chat, status, notifications, schedules (M4) ----
    def _team_context(roles: dict) -> str:
        names = {p["id"]: p["name"] for p in providers_repo.list()}
        feats = []
        for f in features_repo.list():
            b = Budget().status(f["id"])
            feats.append({"title": f["title"], "status": f["status"], "limit": b["limit"], "used": b["used"],
                          "held": b["held"]})
        return team_snapshot(roles, db.kv_get("role_models", {}), names, status_table(list(roles) or ROLES),
                             len(Inbox().list("open")), feats,
                             AgentRegistry().live() if registry.external_enabled() else [])

    # ---- agent registry (FR-59): external agents announce themselves; the human approves them ----
    class RegisterIn(BaseModel):
        name: str
        endpoint: str
        capabilities: list[str] = []
        description: str = ""
        token: str | None = None

    class HeartbeatIn(BaseModel):
        name: str
        token: str

    class TaskToAgent(BaseModel):
        task: str

    @app.get("/api/registry/key", dependencies=guard)
    def get_registry_key():
        return {"key": registry_key()}

    @app.post("/api/registry/register")
    def registry_register(body: RegisterIn, x_registry_key: str | None = Header(None)):
        if not key_ok(x_registry_key):
            raise HTTPException(401, "missing or wrong registry key")
        try:
            out = AgentRegistry().register(body.name, body.endpoint, body.capabilities, body.description, body.token)
        except RegistryError as e:
            raise HTTPException(400, str(e))
        if out["token"]:  # a first registration: the human decides whether this agent may join
            Inbox().post("status", f"External agent wants to join: {out['agent']['name']}",
                         f"Endpoint {out['agent']['endpoint']}. Can: {', '.join(out['agent']['capabilities']) or 'not stated'}. "
                         "Approve or reject it on the Agents page.")
        return {"status": out["agent"]["status"], "token": out["token"]}

    @app.post("/api/registry/heartbeat")
    def registry_heartbeat(body: HeartbeatIn):
        try:
            AgentRegistry().heartbeat(body.name, body.token)
        except RegistryError as e:
            raise HTTPException(401 if "token" in str(e) else 404, str(e))
        return {"ok": True}

    @app.get("/api/registry", dependencies=guard)
    def registry_list():
        return AgentRegistry().list()

    def _registry_decision(fn, name: str):
        try:
            return fn(name)
        except RegistryError as e:
            raise HTTPException(404, str(e))

    @app.post("/api/registry/{name}/approve", dependencies=guard)
    def registry_approve(name: str):
        return _registry_decision(AgentRegistry().approve, name)

    @app.post("/api/registry/{name}/reject", dependencies=guard)
    def registry_reject(name: str):
        return _registry_decision(AgentRegistry().reject, name)

    def _post_to_agent(url: str, payload: dict) -> str:
        client = app.state.http_client or httpx.Client(timeout=30)
        r = client.post(url, json=payload, timeout=30)
        r.raise_for_status()
        try:
            return str(r.json().get("reply", r.text))
        except ValueError:
            return r.text

    @app.post("/api/registry/{name}/task", dependencies=guard)
    def registry_task(name: str, body: TaskToAgent):
        try:
            return AgentRegistry().send_task(name, body.task, _post_to_agent)
        except RegistryError as e:
            raise HTTPException(404 if "unknown" in str(e) else 409, str(e))
        except httpx.HTTPError as e:
            raise HTTPException(502, secrets_store.redact(f"the agent did not answer: {e}"))

    @app.get("/api/team", dependencies=guard)
    def team():
        roles = load_roles(registry.get_settings()["role_dirs"])
        models = db.kv_get("role_models", {})
        names = {p["id"]: p["name"] for p in providers_repo.list()}
        state = {r["role"]: r for r in status_table(list(roles) or ROLES)}
        rows = []
        for name in (list(roles) or ROLES):
            m = models.get(name)
            rows.append({"role": name, "description": roles[name].description if name in roles else "",
                         "tools": roles[name].tools if name in roles else [],
                         "model": f"{names.get(m['provider_id'], 'unknown provider')} / {m['model']}" if m else None,
                         "state": state[name]["state"], "task": state[name]["task"], "steps": state[name]["steps"]})
        return rows

    @app.get("/api/office", dependencies=guard)
    def office():
        """What the office view animates: the team rows plus every hand-off, oldest first.
        A hand-off is real: the Orchestrator delegated a run, or a run finished and reported back."""
        hs = [{"id": h["id"], "from": h["from"], "to": h["to"], "label": h["label"], "at": h["at"]}
              for h in handoffs_repo.list()]
        for r in rt.runs.list():
            if r["status"] in ("done", "failed") and r.get("finished_at"):
                hs.append({"id": "report-" + r["id"], "from": r["role"], "to": "orchestrator",
                           "label": r["status"], "at": r["finished_at"]})
        hs.sort(key=lambda h: h["at"])
        rel = Release()
        pending = [{"id": x["id"], "name": x["name"], "approvals": x.get("approvals", [])}
                   for x in tasks_repo.list() if x.get("release") == "awaiting_prod_approval"]
        return {"team": team(), "handoffs": hs[-40:], "environments": rel.environments(), "pending_prod": pending,
                "docs_gates": [{"id": x["id"], "name": x["name"], "status": docs_state(x)} for x in tasks_repo.list()
                               if x.get("docs_gate")],
                "usage_window": Budget().window(time.time()), "open_incidents": len(Incidents().list(False))}

    @app.post("/api/team/check", dependencies=guard)
    def team_check_endpoint():
        roles = load_roles(registry.get_settings()["role_dirs"])

        def build(pid: str):
            rec = providers_repo.get(pid)
            if not rec:
                raise ProviderError("provider was deleted")
            return registry.build(rec, client=app.state.http_client)
        return team_check(roles or {r: None for r in ROLES}, db.kv_get("role_models", {}), build)

    class _Delegator:
        """What the Orchestrator's delegate / get_report / list_runs tools call. Same rules as POST /api/runs."""

        def __init__(self, roles: dict):
            self.roles = roles

        def ask_agent(self, name, task):
            try:
                return AgentRegistry().send_task(name, task, _post_to_agent)
            except RegistryError as e:
                raise DelegationError(str(e))
            except httpx.HTTPError as e:
                raise DelegationError(secrets_store.redact(f"the agent did not answer: {e}"))

        def delegate(self, role, task, area, done_criteria, context, feature_id, task_id=None):
            task_label = next((ln[6:] for ln in task.splitlines() if ln.startswith("Task: ")), task)[:60]
            if role == "orchestrator":
                raise DelegationError("the orchestrator cannot delegate to itself")
            if role not in self.roles:
                raise DelegationError(f"unknown role {role}; roles are: {', '.join(self.roles)}")
            st = registry.get_settings()
            ws = (st["workspace_roots"] or [str(default_root())])[0]
            sent_at = time.time()  # taken before the run starts, so the report can never be dated earlier
            try:
                run = _launch(role, task, ws, wait=os.environ.get("AO_SYNC_RUNS") == "1", feature_id=feature_id, task_id=task_id)
            except LaunchError as e:
                raise DelegationError(str(e))
            handoffs_repo.create({"from": "orchestrator", "to": role, "label": task_label, "at": sent_at,
                                  "run_id": run["id"]})
            return {"run_id": run["id"], "role": role, "status": run["status"]}

        def request_prod(self, task_id):
            try:
                t = Release().request_prod(task_id)
            except FlowError as e:
                raise DelegationError(str(e))
            return {"task_id": task_id, "release": t["release"], "note": "the human was asked in the inbox"}

        def report(self, run_id):
            run = rt.runs.get(run_id)
            if not run:
                raise DelegationError(f"no run {run_id}")
            return {"run_id": run_id, "role": run["role"], "task": run["task"], "status": run["status"],
                    "steps": run["steps"], "error": run["error"], "result": run.get("result")}

        def runs(self):
            return [{"run_id": r["id"], "role": r["role"], "task": r["task"][:120], "status": r["status"]}
                    for r in rt.runs.list()][-30:]

    @app.post("/api/chat", dependencies=guard)
    def chat(body: ScanIn):
        mapping = db.kv_get("role_models", {}).get("orchestrator")
        rec = providers_repo.get(mapping["provider_id"]) if mapping else None
        if not rec:
            raise HTTPException(400, "no model configured for the orchestrator role")
        roles = load_roles(registry.get_settings()["role_dirs"])
        o = Orchestrator(registry.build(rec, client=app.state.http_client), mapping["model"],
                         roles["orchestrator"].body if "orchestrator" in roles else "", rec["id"],
                         context=_team_context(roles), delegator=_Delegator(roles))
        try:
            return o.chat(body.text)
        except ProviderError as e:
            raise HTTPException(502, secrets_store.redact(f"{e.kind}: {e}"))

    @app.get("/api/chat", dependencies=guard)
    def chat_history():
        return [{"role": m["role"], "content": m["content"], "at": m["at"]}
                for m in db.DocRepo("messages").list() if m.get("chat")]

    @app.get("/api/status", dependencies=guard)
    def status():
        names = list(load_roles(registry.get_settings()["role_dirs"]))
        return status_table(names or ROLES)

    @app.get("/api/notifications", dependencies=guard)
    def notifications(unread: bool = False):
        nt = Notifier()
        return nt.unread() if unread else nt.repo.list()

    @app.post("/api/notifications/{nid}/read", dependencies=guard)
    def read_notification(nid: str):
        n = Notifier().mark_read(nid)
        if not n:
            raise HTTPException(404, "not found")
        return n

    class ScheduleIn(BaseModel):
        name: str
        payload: dict
        run_at: float
        every_s: float | None = None

    @app.get("/api/schedules", dependencies=guard)
    def list_schedules():
        return Scheduler().list()

    @app.post("/api/schedules", dependencies=guard)
    def add_schedule(body: ScheduleIn):
        if body.payload.get("kind") not in JOB_KINDS:
            raise HTTPException(400, f"payload.kind must be one of {sorted(JOB_KINDS)}")
        return Scheduler().add(body.name, body.payload, body.run_at, body.every_s)

    return app


app = create_app()
