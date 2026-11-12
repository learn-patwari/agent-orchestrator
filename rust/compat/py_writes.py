"""Writes a data folder with the Python backend's own code."""
import os
import sys

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
sys.path.insert(0, os.path.join(ROOT, "backend"))
from app import auth, db, secrets_store  # noqa: E402
from app.mcp import McpServers  # noqa: E402
from app.profiles import Profiles  # noqa: E402
from app.scheduler import Scheduler  # noqa: E402

db.init_db()
auth.set_password("correct-horse-1")
p = db.DocRepo("providers").create({"name": "PyProv", "type": "openai", "base_url": "https://py.example/v1",
                                    "api_key_enc": secrets_store.encrypt("sk-py-abcdefghijkl5555")})
db.kv_set("settings", {"merge_style": "rebase"})
db.kv_set("role_models", {"tester": {"provider_id": p["id"], "model": "pm"}})
Profiles().save("pyprof", {"max_steps": 9, "max_total_tokens": 2000, "max_output_tokens": 200, "compact_at_chars": 6000, "keep_recent": 4})
Profiles().set_active("pyprof")
McpServers().save("pysrv", {"transport": "http", "url": "https://mcp.example/rpc"})
Scheduler().add("pynudge", {"kind": "reminder", "text": "hi"}, 4102444800.0, None)
f = db.DocRepo("features").create({"title": "PyFeature", "status": "approved", "skills": []})
db.DocRepo("tasks").create({"feature_id": f["id"], "name": "pytask", "status": "developing", "release": None})
print("written")
