import httpx
import pytest
from fastapi.testclient import TestClient

from app.main import create_app


@pytest.fixture
def env(tmp_path, monkeypatch):
    monkeypatch.setenv("AO_DATA_DIR", str(tmp_path))
    return tmp_path


class Holder:
    handler = staticmethod(lambda req: httpx.Response(500))


@pytest.fixture
def client(env):
    h = Holder()
    transport = httpx.MockTransport(lambda req: h.handler(req))
    app = create_app(http_client=httpx.Client(transport=transport))
    with TestClient(app) as c:
        c.upstream = h
        yield c


@pytest.fixture
def authed(client):
    assert client.post("/api/auth/setup", json={"password": "correct-horse-1"}).status_code == 200
    return client
