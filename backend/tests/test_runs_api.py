import httpx


def completion(text="", tool_calls=None):
    msg = {"content": text or None}
    if tool_calls:
        msg["tool_calls"] = tool_calls
    return httpx.Response(200, json={"model": "m", "usage": {"prompt_tokens": 5, "completion_tokens": 2},
                                     "choices": [{"message": msg}]})


def setup_role(authed, tmp_path):
    roles = tmp_path / "roles"
    roles.mkdir()
    (roles / "developer.md").write_text("---\nname: developer\ndescription: dev\nskills: [ghost]\n---\nYou develop.")
    authed.put("/api/settings", json={"role_dirs": [str(roles)], "skill_paths": [str(tmp_path / "skills")]})
    pid = authed.post("/api/providers", json={"name": "p", "type": "openai", "base_url": "https://x/v1",
                                              "api_key": "sk-test-abcdefghijkl"}).json()["id"]
    return pid


def test_roles_listing_reports_validation_problems(authed, tmp_path):
    setup_role(authed, tmp_path)
    r = authed.get("/api/roles").json()
    assert r[0]["name"] == "developer" and r[0]["problems"] == ["missing skill ghost"]
    assert authed.get("/api/skills").json() == []


def test_run_needs_model_and_workspace(authed, tmp_path):
    pid = setup_role(authed, tmp_path)
    body = {"role": "developer", "task": "t", "workspace": str(tmp_path), "wait": True}
    assert authed.post("/api/runs", json=body).status_code == 400  # no model for role
    authed.put("/api/role-models/developer", json={"provider_id": pid, "model": "m"})
    assert authed.post("/api/runs", json={**body, "workspace": str(tmp_path / "nope")}).status_code == 400
    assert authed.post("/api/runs", json={**body, "role": "ghost"}).status_code == 404


def test_run_end_to_end_via_api(authed, tmp_path):
    pid = setup_role(authed, tmp_path)
    authed.put("/api/role-models/developer", json={"provider_id": pid, "model": "m"})
    ws = tmp_path / "ws"
    ws.mkdir()
    replies = iter([
        completion("", [{"id": "c1", "type": "function", "function": {
            "name": "write_file", "arguments": "{\"path\": \"hello.txt\", \"content\": \"hi\"}"}}]),
        completion("created hello.txt"),
    ])
    authed.upstream.handler = lambda req: next(replies)
    out = authed.post("/api/runs", json={"role": "developer", "task": "make a file", "workspace": str(ws),
                                         "wait": True}).json()
    assert out["status"] == "done" and (ws / "hello.txt").read_text() == "hi"
    detail = authed.get(f"/api/runs/{out['id']}").json()
    assert [m["role"] for m in detail["log"]] == ["user", "assistant", "tool", "assistant"]
    assert authed.get("/api/usage").json()["by_role"]["developer"]["calls"] == 2
    assert authed.get("/api/runs").json()[0]["id"] == out["id"]
