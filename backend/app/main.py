"""FastAPI entry point: auth, settings, providers, role models, price table."""
from contextlib import asynccontextmanager

import threading
from pathlib import Path

from fastapi import Depends, FastAPI, HTTPException, Response
from pydantic import BaseModel

from . import auth, db, secrets_store
from .inbox import Conflict, Inbox
from .providers import pricing, registry
from .providers.base import ProviderError
from .runtime import runner as rt
from .runtime.roles import load_roles, validate
from .runtime.skills import SkillRegistry
from .runtime.tools import ToolBox, Workspace

providers_repo = db.DocRepo("providers")
ROLES = ["orchestrator", "architect", "developer", "tester", "reviewer", "cicd",
         "incident_reporter", "production_support"]


@asynccontextmanager
async def lifespan(app: FastAPI):
    db.init_db()
    yield


def create_app(http_client=None) -> FastAPI:
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
        base_url: str
        api_key: str

    def _public(rec: dict) -> dict:
        out = {k: v for k, v in rec.items() if k != "api_key_enc"}
        out["api_key"] = secrets_store.mask(secrets_store.decrypt(rec["api_key_enc"]))
        return out

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

    @app.post("/api/runs", dependencies=guard)
    def start_run(body: RunIn):
        roles = load_roles(registry.get_settings()["role_dirs"])
        role = roles.get(body.role)
        if not role:
            raise HTTPException(404, f"unknown role {body.role}")
        ws = Path(body.workspace)
        if not ws.is_dir():
            raise HTTPException(400, "workspace must be an existing directory")
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

    @app.post("/api/inbox/{iid}/answer", dependencies=guard)
    def answer_item(iid: str, body: AnswerIn):
        return _inbox_call(Inbox().answer, iid, body.answer)

    @app.post("/api/inbox/{iid}/accept-default", dependencies=guard)
    def accept_default(iid: str):
        return _inbox_call(Inbox().accept_default, iid)

    @app.post("/api/inbox/{iid}/decide", dependencies=guard)
    def decide(iid: str, body: DecideIn):
        return _inbox_call(Inbox().decide, iid, body.approve)

    @app.get("/api/assumptions", dependencies=guard)
    def list_assumptions():
        return Inbox().assumptions()

    @app.post("/api/assumptions/{aid}/overrule", dependencies=guard)
    def overrule(aid: str, body: OverruleIn):
        return _inbox_call(Inbox().overrule, aid, body.value)

    @app.get("/api/audit", dependencies=guard)
    def audit():
        return db.DocRepo("audit").list()

    return app


app = create_app()
