from app import db, secrets_store


def test_requires_auth(client):
    assert client.get("/api/settings").status_code == 401
    assert client.get("/api/providers").status_code == 401
    assert client.get("/api/health").status_code == 200


def test_setup_login_flow(client):
    assert client.get("/api/auth/status").json() == {"setup_done": False}
    assert client.post("/api/auth/setup", json={"password": "short"}).status_code == 400
    assert client.post("/api/auth/setup", json={"password": "correct-horse-1"}).status_code == 200
    assert client.get("/api/settings").status_code == 200  # cookie set by setup
    assert client.post("/api/auth/setup", json={"password": "another-pass-2"}).status_code == 409
    client.post("/api/auth/logout")
    client.cookies.clear()
    assert client.get("/api/settings").status_code == 401
    assert client.post("/api/auth/login", json={"password": "wrong-pass-123"}).status_code == 401
    assert client.post("/api/auth/login", json={"password": "correct-horse-1"}).status_code == 200
    assert client.get("/api/settings").status_code == 200


def test_tampered_token_rejected(authed):
    authed.cookies.set("ao_session", "9999999999.abcd.deadbeef")
    assert authed.get("/api/settings").status_code == 401


def test_settings_defaults_and_flags(authed):
    s = authed.get("/api/settings").json()
    assert s["feature_flags"]["external_providers"] is False
    assert s["integration_branch"] == "internal"
    s = authed.put("/api/settings", json={"merge_style": "squash"}).json()
    assert s["merge_style"] == "squash"
    assert authed.put("/api/settings", json={"bogus": 1}).status_code == 400


def test_persistence_models_exist(authed):
    for t in db.TABLES:
        repo = db.DocRepo(t)
        d = repo.create({"x": 1})
        assert repo.get(d["id"])["x"] == 1
        assert repo.update(d["id"], {"x": 2})["x"] == 2
        assert repo.delete(d["id"]) and repo.get(d["id"]) is None


def test_secret_helpers_and_scan(authed):
    enc = secrets_store.encrypt("sk-abcdefghijklmnopqrstuvwxyz")
    assert "sk-abc" not in enc and secrets_store.decrypt(enc).startswith("sk-abc")
    assert "[REDACTED]" in secrets_store.redact("key sk-abcdefghijklmnopqrstuvwxyz here")
    r = authed.post("/api/secrets/scan", json={"text": "my password: hunter2hunter2"}).json()
    assert r["looks_like_secret"] is True
    assert authed.post("/api/secrets/scan", json={"text": "build a login page"}).json()["looks_like_secret"] is False
