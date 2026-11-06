"""Runtime configuration. Everything local-first; data lives under AO_DATA_DIR."""
import os
from pathlib import Path


def data_dir() -> Path:
    d = Path(os.environ.get("AO_DATA_DIR", Path(__file__).resolve().parents[2] / "data"))
    d.mkdir(parents=True, exist_ok=True)
    return d


def db_path() -> Path:
    return data_dir() / "orchestrator.db"


DEFAULT_SETTINGS = {
    "feature_flags": {"external_providers": False},
    "merge_style": "merge",  # merge | squash | rebase (A-7)
    "integration_branch": "internal",
    "stage_branch": "stage",
    "main_branch": "main",
    "attribution_trailer": "",  # A-16, empty = off
    "review_round_limit": 3,  # Q-6
    "decide_with_defaults": False,
    "skill_paths": [str(Path.home() / ".claude" / "skills")],  # precedence: first wins
    "role_dirs": [],
    "run_limits": {"max_steps": 30, "max_total_tokens": 500000},
}
