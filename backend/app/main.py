"""FastAPI entry point: auth, settings, providers, role models, price table."""
from contextlib import asynccontextmanager

import os
import threading
from pathlib import Path

from fastapi import Depends, FastAPI, HTTPException, Response
from fastapi.responses import PlainTextResponse
from pydantic import BaseModel

from . import auth, db, secrets_store
from .ba import BAError
from .features import Features, NotApproved, features as features_repo, require_approved
from .inbox import Conflict, Inbox
from .notify import Notifier
from .pipeline.flow import Flow, FlowError, tasks as tasks_repo
from .pipeline.github import GhClient
from .pipeline.merge_exec import MergeRefused, merge_batch
from .pipeline.merge_gate import GateConfig
from .pipeline.worktrees import WorktreeError, WorktreeManager
from .orchestrator import Orchestrator, check_workspace, default_root, status_table
from .scheduler import Scheduler, budget_percent_used
from .providers import pricing, registry
from .providers.presets import PRESETS
from .providers.base import ProviderError
from .runtime import runner as rt
from .runtime.roles import load_roles, validate
from .runtime.skills import SkillRegistry
from .runtime.tools import ToolBox, Workspace

providers_repo = db.DocRepo("providers")
ROLES = ["orchestrator", "architect", "developer", "tester", "reviewer", "cicd",
         "incident_reporter", "production_support"]


JOB_KINDS = {"reminder"}  # more kinds (start a run, resume a lane) arrive with the pipeline milestones


def run_job(job: dict) -> None:
    if job["payload"]["kind"] == "reminder":
        Inbox().post("status", job["name"], job["payload"].get("text", ""))
        Notifier().send("reminder", job["name"], job["payload"].get("text", ""), key=f"job:{job['id']}")


def scheduler_loop(stop: threading.Event, every: float = 30.0) -> None:
    """Local background loop: schedules run only while the app is running."""
    import time as _t
    while not stop.wait(every):
        try:
            st = registry.get_settings()
            Scheduler(st["pause_at_percent"]).tick(_t.time(), budget_percent_used(st["daily_budget_usd"]), run_job)
        except Exception:
            pass  # never let the loop die; failures of single jobs are recorded by tick()


@asynccontextmanager
async def lifespan(app: FastAPI):
    db.init_db()
    stop = threading.Event()
    if not os.environ.get("AO_NO_SCHEDULER"):
        threading.Thread(target=scheduler_loop, args=(stop,), daemon=True).start()
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
            "name": body.name, "type": body.type, "base_url": body.base_url.rstrip("/"),
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
    @app.get("/api/role-models", dependencies=guard)
    def get_role_models():
        return {"roles": ROLES, "models": db.kv_get("role_models", {})}

    @app.put("/api/role-models/{role}", dependencies=guard)
    def set_role_model(role: str, body: dict):
        if role not in ROLES:
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

    @app.post("/api/runs", dependencies=guard)
    def start_run(body: RunIn):
        if body.role == "developer":  # FR-33: no development before the human approved the SRS
            try:
                require_approved(body.feature_id)
            except NotApproved as e:
                raise HTTPException(409, str(e))
        roles = load_roles(registry.get_settings()["role_dirs"])
        role = roles.get(body.role)
        if not role:
            raise HTTPException(404, f"unknown role {body.role}")
        st = registry.get_settings()
        try:
            ws = check_workspace(body.workspace, st["workspace_roots"] or [str(default_root())])
        except ValueError as e:
            raise HTTPException(400, str(e))
        pct = budget_percent_used(st["daily_budget_usd"])
        if pct is not None and pct >= 100:
            Notifier().send("budget_pause", "Daily LLM budget reached",
                            f"Used {pct:.0f}% of the daily cap; new runs are blocked.", key="budget_pause")
            raise HTTPException(429, "daily LLM budget reached; new runs are blocked")
        mapping = db.kv_get("role_models", {}).get(body.role)
        if not mapping:
            raise HTTPException(400, f"no model configured for role {body.role}")
        rec = providers_repo.get(mapping["provider_id"])
        if not rec:
            raise HTTPException(400, "provider for this role no longer exists")
        try:
            provider = registry.build(rec, client=app.state.http_client)
        except ValueError as e:
            raise HTTPException(400, str(e))
        reg = _registry()
        lim = registry.get_settings()["run_limits"]
        runner = rt.Runner(provider, mapping["model"], role, ToolBox(Workspace(ws), reg), reg,
                           rt.Limits(max_steps=lim["max_steps"], max_total_tokens=lim["max_total_tokens"]),
                           provider_id=rec["id"])
        run = runner.start(body.task, str(ws))
        if body.wait:
            return runner.run(run["id"])
        threading.Thread(target=runner.run, args=(run["id"],), daemon=True).start()
        return run

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
        return _flow_call(_flow().start, body.feature_id, body.name, body.stacked_on)

    @app.post("/api/pipeline/tasks/{tid}/dev-done", dependencies=guard)
    def task_dev_done(tid: str, body: DevDoneIn):
        return _flow_call(_flow().dev_done, tid, body.pr_number, body.head_sha)

    @app.post("/api/pipeline/tasks/{tid}/tests", dependencies=guard)
    def task_tests(tid: str, body: TestsIn):
        return _flow_call(_flow().record_tests, tid, body.passed, body.failed, body.area)

    @app.post("/api/pipeline/tasks/{tid}/review", dependencies=guard)
    def task_review(tid: str, body: ReviewIn):
        return _flow_call(_flow().record_review, tid, body.body)

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
    @app.post("/api/chat", dependencies=guard)
    def chat(body: ScanIn):
        mapping = db.kv_get("role_models", {}).get("orchestrator")
        rec = providers_repo.get(mapping["provider_id"]) if mapping else None
        if not rec:
            raise HTTPException(400, "no model configured for the orchestrator role")
        roles = load_roles(registry.get_settings()["role_dirs"])
        o = Orchestrator(registry.build(rec, client=app.state.http_client), mapping["model"],
                         roles["orchestrator"].body if "orchestrator" in roles else "", rec["id"])
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
