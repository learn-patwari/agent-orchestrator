import httpx

OK = {"model": "m1", "usage": {"prompt_tokens": 4, "completion_tokens": 2},
      "choices": [{"message": {"content": "ok"}}]}


def add(authed, type="openai"):
    return authed.post("/api/providers", json={
        "name": "p", "type": type, "base_url": "https://llm.example/v1/", "api_key": "sk-secret-abcdefghijkl1234"})


def test_provider_key_never_returned_and_encrypted_at_rest(authed, env):
    r = add(authed)
    assert r.status_code == 200
    body = r.json()
    assert body["api_key"] == "****1234" and body["base_url"] == "https://llm.example/v1"
    assert "api_key_enc" not in body
    listed = authed.get("/api/providers").json()
    assert "sk-secret" not in str(listed)
    from app.db import DocRepo
    assert "sk-secret" not in str(DocRepo("providers").list())  # stored encrypted


def test_test_connection_success_and_auth_failure(authed):
    pid = add(authed).json()["id"]
    authed.upstream.handler = lambda req: httpx.Response(200, json=OK)
    r = authed.post(f"/api/providers/{pid}/test", json={"model": "m1"}).json()
    assert r["ok"] is True and r["usage"] == {"input_tokens": 4, "output_tokens": 2}
    authed.upstream.handler = lambda req: httpx.Response(401, text="bad key sk-secret-abcdefghijkl1234")
    r = authed.post(f"/api/providers/{pid}/test", json={"model": "m1"}).json()
    assert r["ok"] is False and r["error_kind"] == "auth"
    assert "sk-secret" not in r["error"]  # redacted


def test_external_providers_hidden_behind_flag(authed):
    assert authed.get("/api/providers/types").json()["types"] == ["openai", "anthropic", "claude_cli"]
    assert add(authed, "external").status_code == 400
    authed.put("/api/settings", json={"feature_flags": {"external_providers": True}})
    assert "external" in authed.get("/api/providers/types").json()["types"]
    authed.put("/api/settings", json={"feature_flags": {"external_providers": False}})
    assert add(authed, "external").status_code == 400


def test_role_models_and_usage_cost(authed):
    pid = add(authed).json()["id"]
    assert authed.put("/api/role-models/developer", json={"provider_id": "nope", "model": "m"}).status_code == 400
    assert authed.put("/api/role-models/ghost", json={"provider_id": pid, "model": "m"}).status_code == 404
    m = authed.put("/api/role-models/developer", json={"provider_id": pid, "model": "m1"}).json()
    assert m["developer"]["model"] == "m1"
    assert authed.put("/api/prices", json={"m1": {"input": 1.0}}).status_code == 400
    authed.put("/api/prices", json={"m1": {"input": 2.0, "output": 4.0}})
    from app.providers.base import Usage
    from app.providers.registry import record_call
    record_call("developer", pid, "m1", Usage(1_000_000, 500_000))
    u = authed.get("/api/usage").json()["by_role"]["developer"]
    assert u["calls"] == 1 and u["cost_usd"] == 4.0


def test_unexpected_adapter_crash_is_reported_not_a_500(authed, monkeypatch):
    from app.providers import registry

    class Boom:
        def test_connection(self, model):
            raise RuntimeError("boom sk-secret-abcdefghijkl1234")
    pid = add(authed).json()["id"]
    monkeypatch.setattr(registry, "build", lambda rec, client=None: Boom())
    r = authed.post(f"/api/providers/{pid}/test", json={"model": "m"})
    assert r.status_code == 200
    body = r.json()
    assert body["ok"] is False and "boom" in body["error"] and "sk-secret" not in body["error"]
