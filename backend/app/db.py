"""SQLite persistence behind a small repository interface (A-1, Q-1).

Every entity is a JSON document in its own table, so a Mongo implementation of
`DocRepo` can replace this later without touching callers.
"""
import json
import sqlite3
import time
import uuid
from contextlib import contextmanager

from .config import db_path

TABLES = [
    "users", "projects", "features", "tasks", "agents", "runs", "messages",
    "approvals", "assumptions", "budgets", "providers", "mcp_servers",
    "plugins", "llm_calls", "inbox", "audit", "notifications", "schedules", "srs_versions", "handoffs", "incidents", "ext_agents",
]


@contextmanager
def connect():
    con = sqlite3.connect(db_path())
    con.row_factory = sqlite3.Row
    try:
        yield con
        con.commit()
    finally:
        con.close()


def init_db() -> None:
    with connect() as con:
        for t in TABLES:
            con.execute(
                f"CREATE TABLE IF NOT EXISTS {t} (id TEXT PRIMARY KEY, created_at REAL NOT NULL, data TEXT NOT NULL)"
            )
        con.execute("CREATE TABLE IF NOT EXISTS kv (k TEXT PRIMARY KEY, v TEXT NOT NULL)")


class DocRepo:
    def __init__(self, table: str):
        if table not in TABLES:
            raise ValueError(f"unknown table {table}")
        self.t = table

    def create(self, doc: dict, id: str | None = None) -> dict:
        id = id or uuid.uuid4().hex
        doc = {**doc, "id": id}
        with connect() as con:
            con.execute(
                f"INSERT INTO {self.t} (id, created_at, data) VALUES (?,?,?)",
                (id, time.time(), json.dumps(doc)),
            )
        return doc

    def get(self, id: str) -> dict | None:
        with connect() as con:
            r = con.execute(f"SELECT data FROM {self.t} WHERE id=?", (id,)).fetchone()
        return json.loads(r["data"]) if r else None

    def list(self) -> list[dict]:
        with connect() as con:
            rows = con.execute(f"SELECT data FROM {self.t} ORDER BY created_at").fetchall()
        return [json.loads(r["data"]) for r in rows]

    def update(self, id: str, patch: dict) -> dict | None:
        cur = self.get(id)
        if cur is None:
            return None
        cur = {**cur, **patch, "id": id}
        with connect() as con:
            con.execute(f"UPDATE {self.t} SET data=? WHERE id=?", (json.dumps(cur), id))
        return cur

    def delete(self, id: str) -> bool:
        with connect() as con:
            n = con.execute(f"DELETE FROM {self.t} WHERE id=?", (id,)).rowcount
        return n > 0


def kv_get(key: str, default=None):
    with connect() as con:
        r = con.execute("SELECT v FROM kv WHERE k=?", (key,)).fetchone()
    return json.loads(r["v"]) if r else default


def kv_set(key: str, value) -> None:
    with connect() as con:
        con.execute("INSERT INTO kv (k,v) VALUES (?,?) ON CONFLICT(k) DO UPDATE SET v=excluded.v",
                    (key, json.dumps(value)))
