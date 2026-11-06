import pytest

from app.ba import BAError, build_srs, lanes, parse_json


def test_parse_json_handles_fences_and_prose():
    assert parse_json('```json\n{"a": 1}\n```') == {"a": 1}
    assert parse_json('Sure! Here it is: {"a": [1, 2]} hope that helps') == {"a": [1, 2]}
    for bad in ("no json here", '{"a": ', ""):
        with pytest.raises(BAError):
            parse_json(bad)


def T(id, files=(), deps=()):
    return {"id": id, "title": id, "files": list(files), "depends_on": list(deps)}


def test_independent_tasks_get_separate_lanes():
    plan = [{"id": "P1", "name": "x", "tasks": [T("a", ["f1"]), T("b", ["f2"]), T("c", ["f3"])]}]
    out = lanes(plan)
    assert len({t["lane"] for t in out[0]["tasks"]}) == 3
    assert all(t["stacked_on"] is None for t in out[0]["tasks"])


def test_tasks_sharing_a_file_share_a_lane_and_stack_in_order():
    plan = [{"id": "P1", "name": "x", "tasks": [T("a", ["server.py"]), T("b", ["server.py", "x"]), T("c", ["y"])]}]
    t = {x["id"]: x for x in lanes(plan)[0]["tasks"]}
    assert t["a"]["lane"] == t["b"]["lane"] != t["c"]["lane"]
    assert t["a"]["stacked_on"] is None and t["b"]["stacked_on"] == "a"


def test_shared_file_is_transitive_across_tasks():
    plan = [{"id": "P1", "name": "x", "tasks": [T("a", ["f1"]), T("b", ["f2"]), T("c", ["f1", "f2"])]}]
    assert len({x["lane"] for x in lanes(plan)[0]["tasks"]}) == 1  # c bridges a and b


def test_dependencies_force_later_wave_and_cycles_are_rejected():
    plan = [{"id": "P1", "name": "x", "tasks": [T("a", ["f1"]), T("b", ["f2"], ["a"])]}]
    t = {x["id"]: x for x in lanes(plan)[0]["tasks"]}
    assert t["a"]["wave"] == 0 and t["b"]["wave"] == 1
    with pytest.raises(BAError, match="cycle"):
        lanes([{"id": "P", "name": "x", "tasks": [T("a", [], ["b"]), T("b", [], ["a"])]}])
    with pytest.raises(BAError, match="unknown"):
        lanes([{"id": "P", "name": "x", "tasks": [T("a", [], ["zzz"])]}])


FEATURE = {
    "title": "Tax report", "text": "Add a tax report page",
    "analysis": {"restatement": "A page that lists taxes.", "why": "Finance needs it.",
                 "impact": {"modules": ["reports"], "data": ["invoices"], "security": ["role checks"],
                            "risks": ["slow query"]}},
    "qa": [{"question": "DB?", "answer": "sqlite", "by": "human"},
           {"question": "Format?", "answer": "csv", "by": "accepted_default"},
           {"question": "Locale?", "answer": "en", "by": "auto_default"}],
    "plan": [{"id": "P1", "name": "Backend", "tasks": [T("a", ["api.py"])]}],
}


def test_srs_follows_template_with_decisions_assumptions_and_signoff():
    md = build_srs(FEATURE)
    for h in ("# SRS: Tax report", "## 1. Purpose and background", "## 2. Decisions and defaults",
              "## 10. Development plan", "## 11. Sign-off"):
        assert h in md
    assert "| D-1 | DB?: sqlite |" in md
    assert "| A-1 | Format?: csv **[A]**" in md and "| A-2 | Locale?: en **[A]**" in md
    assert "Status: Draft" in md and "slow query" in md and "api.py" in md
    assert "Approved by" in md  # empty sign-off block for the human


def test_srs_with_no_questions_still_valid():
    md = build_srs({**FEATURE, "qa": []})
    assert "| D-1" not in md and "No open decisions" in md
