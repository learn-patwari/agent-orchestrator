"""Reads, with the Python backend's own code, a data folder the Rust backend wrote."""
import json
import os
import sys

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
sys.path.insert(0, os.path.join(ROOT, "backend"))
from app import auth, db, secrets_store  # noqa: E402
from app.mcp import McpServers  # noqa: E402
from app.profiles import Profiles  # noqa: E402
from app.providers import registry  # noqa: E402
from app.scheduler import Scheduler  # noqa: E402

out = {}
out["password_ok"] = auth.verify_password("correct-horse-1")
out["password_wrong"] = auth.verify_password("nope-nope-nope")
tok = auth.make_token()
out["token_roundtrip"] = auth.valid_token(tok)
p = db.DocRepo("providers").list()[0]
out["provider_name"] = p["name"]
out["provider_key_decrypts"] = secrets_store.decrypt(p["api_key_enc"])
out["settings_merge_style"] = registry.get_settings()["merge_style"]
out["role_model"] = db.kv_get("role_models", {}).get("developer_1")
out["profiles_active"] = Profiles().active()["name"]
out["profile_mine"] = Profiles().get("mine")["max_steps"]
out["mcp"] = [s["name"] for s in McpServers().list()]
out["schedules"] = [s["name"] for s in Scheduler().list()]
print(json.dumps(out))
