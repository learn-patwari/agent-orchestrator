from app.inbox import Inbox, request_gate


def test_inbox_requires_auth(client):
    assert client.get("/api/inbox").status_code == 401
    assert client.get("/api/assumptions").status_code == 401
    assert client.get("/api/audit").status_code == 401


def test_question_flow_over_api(authed):
    q = Inbox().ask("DB?", "Which?", options=["sqlite", "mongo"], default="sqlite")
    assert [i["id"] for i in authed.get("/api/inbox?status=open").json()] == [q["id"]]
    r = authed.post(f"/api/inbox/{q['id']}/accept-default")
    assert r.status_code == 200 and r.json()["answer"] == "sqlite"
    a = authed.get("/api/assumptions").json()
    assert a[0]["assumed"] == "sqlite"
    o = authed.post(f"/api/assumptions/{a[0]['id']}/overrule", json={"value": "mongo"}).json()
    assert o["status"] == "overruled" and o["overruled_to"] == "mongo"
    assert authed.post(f"/api/inbox/{q['id']}/answer", json={"answer": "x"}).status_code == 409  # already resolved
    assert authed.post("/api/inbox/nope/answer", json={"answer": "x"}).status_code == 404
    assert authed.post(f"/api/inbox/{Inbox().ask('n', 'n')['id']}/accept-default").status_code == 400


def test_decide_with_defaults_setting_applies_to_new_questions_only(authed):
    from app.providers.registry import get_settings
    authed.put("/api/settings", json={"decide_with_defaults": True})
    q = Inbox(decide_with_defaults=get_settings()["decide_with_defaults"]).ask("x", "x", default="a")
    assert q["status"] == "resolved"


def test_gate_decision_over_api_and_audit(authed):
    g = request_gate("merge_main", "merge PR 5", run_id="r1")
    assert authed.post(f"/api/inbox/{g['id']}/answer", json={"answer": "yes"}).status_code == 400  # approvals use /decide
    r = authed.post(f"/api/inbox/{g['id']}/decide", json={"approve": True})
    assert r.status_code == 200 and r.json()["status"] == "approved"
    events = [e["event"] for e in authed.get("/api/audit").json()]
    assert events == ["gate_requested", "gate_approved"]
