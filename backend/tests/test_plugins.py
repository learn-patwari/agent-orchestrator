"""Plugins: a folder of roles, skills, workflows, templates and MCP config that is validated on load and installed from the UI."""
import json
from pathlib import Path

import pytest

from app import db
from app.plugins import SHIPPED, Plugins, validate_plugin

TOOLS = {"read_file", "write_file", "use_skill", "list_dir"}
ROLES = {"orchestrator", "developer_1"}
SKILLS = {"srs-writer"}


def make(root, name="demo", manifest=True, role_skills="[]", role_tools="[read_file]", workflow=None, mcp=None):
    p = root / name
    (p / "roles").mkdir(parents=True)
    (p / "skills" / "pg").mkdir(parents=True)
    (p / "skills" / "pg" / "SKILL.md").write_text("---\nname: pg\ndescription: postgres tips\n---\nUse indexes.")
    (p / "roles" / "dba.md").write_text(
        f"---\nname: dba\ndescription: database person\ntools: {role_tools}\nskills: {role_skills}\n---\nYou tune databases.")
    if manifest:
        (p / "plugin.json").write_text(json.dumps({"name": name, "version": "1.0.0", "description": "demo plugin"}))
    if workflow:
        (p / "workflows").mkdir()
        (p / "workflows" / "flow.yaml").write_text(workflow)
    if mcp is not None:
        (p / "mcp.json").write_text(json.dumps(mcp))
    return p


def check(p):
    return validate_plugin(p, ROLES, TOOLS, SKILLS)


def test_a_valid_plugin_has_no_problems(tmp_path):
    assert check(make(tmp_path, role_skills="[pg]")) == []


def test_a_folder_without_a_manifest_is_rejected_with_a_reason(tmp_path):
    assert any("plugin.json" in x for x in check(make(tmp_path, manifest=False)))


def test_manifest_fields_are_required(tmp_path):
    p = make(tmp_path)
    (p / "plugin.json").write_text(json.dumps({"name": "demo"}))
    problems = check(p)
    assert any("version" in x for x in problems) and any("description" in x for x in problems)
    (p / "plugin.json").write_text("{not json")
    assert any("plugin.json" in x and "JSON" in x for x in check(p))


def test_a_role_naming_a_missing_skill_fails_with_file_and_line(tmp_path):
    problems = check(make(tmp_path, role_skills="[pg, ghost]"))
    hit = [x for x in problems if "ghost" in x]
    assert hit and "dba.md:5" in hit[0].replace("\\", "/")  # the skills line of the role file


def test_skills_from_the_app_count_as_known_and_unknown_tools_are_reported(tmp_path):
    assert check(make(tmp_path, role_skills="[srs-writer]")) == []
    assert any("unknown tool flying" in x for x in check(make(tmp_path / "b", role_tools="[read_file, flying]")))


def test_workflow_stages_must_name_known_roles(tmp_path):
    ok = "stages:\n  - {name: build, role: dba}\n  - {name: review, role: developer_1}\n"
    bad = "stages:\n  - {name: build, role: dba}\n  - {name: ship, role: wizard}\n"
    assert check(make(tmp_path / "a", workflow=ok)) == []
    problems = check(make(tmp_path / "b", workflow=bad))
    assert any("wizard" in x and "flow.yaml" in x for x in problems)


def test_mcp_config_may_not_carry_secrets(tmp_path):
    clean = {"servers": {"jira": {"transport": "http", "url": "https://x.example", "roles": ["dba"],
                                  "env_refs": {"TOKEN": "JIRA_TOKEN"}}}}
    leaky = {"servers": {"jira": {"transport": "http", "url": "https://x.example", "roles": ["dba"],
                                  "env": {"API_TOKEN": "sk-live-abcdefghijklmnop"}}}}
    assert check(make(tmp_path / "a", mcp=clean)) == []
    assert any("secret" in x.lower() for x in check(make(tmp_path / "b", mcp=leaky)))


@pytest.fixture
def mgr(env):
    db.init_db()
    return Plugins()


def test_install_enable_disable_remove_cycle_and_a_broken_plugin_stays_disabled(mgr, tmp_path):
    good, bad = make(tmp_path, "good", role_skills="[pg]"), make(tmp_path, "bad", role_skills="[ghost]")
    g = mgr.install(str(good), ROLES, TOOLS, SKILLS)
    assert g["problems"] == [] and g["enabled"] is False
    b = mgr.install(str(bad), ROLES, TOOLS, SKILLS)
    assert b["problems"] and b["enabled"] is False
    with pytest.raises(ValueError, match="problems"):
        mgr.enable("bad", ROLES, TOOLS, SKILLS)
    assert mgr.enable("good", ROLES, TOOLS, SKILLS)["enabled"] is True
    role_dirs, skill_dirs = mgr.dirs()
    assert [Path(d).parent.name for d in role_dirs] == ["good"] and [Path(d).parent.name for d in skill_dirs] == ["good"]
    assert mgr.disable("good")["enabled"] is False and mgr.dirs() == ([], [])
    assert mgr.remove("good") and not mgr.remove("good")
    assert [p["name"] for p in mgr.list()] == ["bad"]


def test_installing_a_folder_that_is_not_a_plugin_is_refused(mgr, tmp_path):
    with pytest.raises(ValueError, match="plugin.json"):
        mgr.install(str(tmp_path), ROLES, TOOLS, SKILLS)
    with pytest.raises(ValueError):
        mgr.install(str(tmp_path / "nowhere"), ROLES, TOOLS, SKILLS)


def test_the_shipped_software_delivery_plugin_is_valid():
    assert (SHIPPED / "software-delivery" / "plugin.json").exists()
    from app.runtime.roles import DEFAULT_TOOLS
    from app.runtime.tools import ToolBox, Workspace
    from app.config import BUILTIN
    tools = set(ToolBox(Workspace(Path(".")), None).specs)
    skills = {p.parent.name for p in (BUILTIN / "skills").glob("*/SKILL.md")}
    roles = {p.stem for p in (BUILTIN / "roles").glob("*.md")}
    assert validate_plugin(SHIPPED / "software-delivery", roles, tools, skills) == []


def test_plugin_api_installs_enables_and_the_roles_and_skills_appear(authed, tmp_path):
    p = make(tmp_path, "dbtools", role_skills="[pg]")
    assert authed.post("/api/plugins/install", json={"path": str(p)}).json()["problems"] == []
    assert authed.post("/api/plugins/install", json={"path": str(tmp_path / "ghost")}).status_code == 400
    roles = {r["name"] for r in authed.get("/api/roles").json()}
    assert "dba" not in roles  # installed but not enabled
    assert authed.post("/api/plugins/dbtools/enable").json()["enabled"] is True
    roles = {r["name"]: r for r in authed.get("/api/roles").json()}
    assert "dba" in roles and roles["dba"]["problems"] == []  # its skill "pg" is found through the plugin
    names = {x["name"] for x in authed.get("/api/plugins").json()["installed"]}
    assert names == {"dbtools"}
    assert "software-delivery" in {x["name"] for x in authed.get("/api/plugins").json()["available"]}
    assert authed.post("/api/plugins/ghost/enable").status_code == 404
    assert authed.post("/api/plugins/dbtools/disable").json()["enabled"] is False
    assert "dba" not in {r["name"] for r in authed.get("/api/roles").json()}
    assert authed.delete("/api/plugins/dbtools").status_code == 200
    assert authed.delete("/api/plugins/dbtools").status_code == 404
