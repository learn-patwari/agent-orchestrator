import json
import os
import re
import sys

os.environ["AO_DATA_DIR"] = os.environ.get("TEMP", ".") + r"\ao-routes"
os.environ["AO_NO_SCHEDULER"] = "1"
ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
sys.path.insert(0, os.path.join(ROOT, "backend"))
from app.main import create_app  # noqa: E402

py = set()
for r in create_app().routes:
    if getattr(r, "path", "").startswith("/api/"):
        for m in r.methods - {"HEAD", "OPTIONS"}:
            py.add((m, re.sub(r"\{(\w+)\}", r":\1", r.path)))

rs = set()
root = os.path.join(ROOT, "rust", "ao-server", "src", "routes")
for f in os.listdir(root):
    for m in re.finditer(r'\.(?:guarded|open)\(Method::(\w+), "([^"]+)"', open(os.path.join(root, f), encoding="utf-8").read()):
        rs.add((m.group(1), m.group(2)))

print(json.dumps({"python": len(py), "rust": len(rs), "only_python": sorted(py - rs), "only_rust": sorted(rs - py)}))
