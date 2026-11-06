import json

import httpx

from tests.test_features import ANALYSIS, PLAN
from tests.test_runs_api import completion, setup_role


def reply(obj):
    return lambda req: completion(json.dumps(obj))


def arch(authed):
    pid = authed.post("/api/providers", json={"name": "p", "type": "openai", "base_url": "https://x/v1",
                                              "api_key": "sk-test-abcdefghijkl"}).json()["id"]
    authed.put("/api/role-models/architect", json={"provider_id": pid, "model": "m"})
    return pid


def test_endpoints_require_auth(client):
    assert client.get("/api/features").status_code == 401
    assert client.post("/api/features", json={"title": "t", "text": "x"}).status_code == 401


def test_intake_needs_architect_model(authed):
    assert authed.post("/api/features", json={"title": "t", "text": "x"}).status_code == 400


def test_full_flow_over_api_to_approval(authed):
    arch(authed)
    authed.upstream.handler = reply(ANALYSIS)
    f = authed.post("/api/features", json={"title": "Tax report", "text": "Add a tax report"}).json()
    assert f["status"] == "questioning" and f["analysis"]["restatement"]
    q = authed.get("/api/inbox?status=open").json()
    assert [i["title"] for i in q] == ["DB?"]
    authed.post(f"/api/inbox/{q[0]['id']}/answer", json={"answer": "mongo"})
    q = authed.get("/api/inbox?status=open").json()
    assert [i["title"] for i in q] == ["Format?"]  # next question appeared by itself
    authed.post(f"/api/inbox/{q[0]['id']}/accept-default")
    assert authed.get(f"/api/features/{f['id']}").json()["status"] == "ready"
    authed.upstream.handler = reply(PLAN)
    fin = authed.post(f"/api/features/{f['id']}/finalize")
    assert fin.status_code == 200 and fin.json()["status"] == "srs_draft"
    srs = authed.get(f"/api/features/{f['id']}/srs")
    assert "# SRS: Tax report" in srs.text and "D-1" in srs.text and "[A]" in srs.text
    gate = [i for i in authed.get("/api/inbox?status=open").json() if i["kind"] == "approval"][0]
    assert authed.post(f"/api/inbox/{gate['id']}/decide", json={"approve": True}).status_code == 200
    assert authed.get(f"/api/features/{f['id']}").json()["status"] == "approved"
    assert [x["id"] for x in authed.get("/api/features").json()] == [f["id"]]


def test_finalize_early_is_409_and_unknown_feature_404(authed):
    arch(authed)
    authed.upstream.handler = reply(ANALYSIS)
    f = authed.post("/api/features", json={"title": "t", "text": "x"}).json()
    assert authed.post(f"/api/features/{f['id']}/finalize").status_code == 409
    assert authed.get("/api/features/nope").status_code == 404


def test_bad_model_output_is_a_502_not_a_crash(authed):
    arch(authed)
    authed.upstream.handler = lambda req: completion("I cannot help with that")
    r = authed.post("/api/features", json={"title": "t", "text": "x"})
    assert r.status_code == 502 and authed.get("/api/features").json() == []


def test_developer_run_requires_an_approved_feature(authed, tmp_path):
    pid = setup_role(authed, tmp_path)
    authed.put("/api/role-models/developer", json={"provider_id": pid, "model": "m"})
    from app.db import DocRepo
    ok = DocRepo("features").create({"title": "a", "status": "approved"})
    draft = DocRepo("features").create({"title": "b", "status": "srs_draft"})
    authed.upstream.handler = lambda req: completion("done")
    body = {"role": "developer", "task": "t", "workspace": str(tmp_path), "wait": True}
    assert authed.post("/api/runs", json=body).status_code == 409
    assert authed.post("/api/runs", json={**body, "feature_id": draft["id"]}).status_code == 409
    assert authed.post("/api/runs", json={**body, "feature_id": "nope"}).status_code == 409
    assert authed.post("/api/runs", json={**body, "feature_id": ok["id"]}).json()["status"] == "done"
