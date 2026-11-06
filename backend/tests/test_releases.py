import pytest

from app.releases import generate_notes, next_version, suggest_bump


def test_breaking_change_suggests_major():
    assert suggest_bump(["feat: add x", "fix!: drop old api"]) == "major"
    assert suggest_bump(["feat: add x\n\nBREAKING CHANGE: removed y"]) == "major"


def test_feature_suggests_minor_and_fix_patch():
    assert suggest_bump(["fix: typo", "feat(ui): button"]) == "minor"
    assert suggest_bump(["fix: typo", "docs: readme"]) == "patch"


def test_no_commits_suggests_nothing():
    assert suggest_bump([]) is None


def test_next_version_bumps():
    assert next_version("1.2.3", "major") == "2.0.0"
    assert next_version("1.2.3", "minor") == "1.3.0"
    assert next_version("1.2.3", "patch") == "1.2.4"
    assert next_version("v0.9.0", "minor") == "0.10.0"
    assert next_version(None, "minor") == "0.1.0"


def test_next_version_rejects_bad_input():
    with pytest.raises(ValueError):
        next_version("1.2", "patch")
    with pytest.raises(ValueError):
        next_version("1.2.3", "huge")


def test_notes_group_by_type_and_list_breaking_first():
    notes = generate_notes("1.3.0", ["feat: add export", "fix: crash on empty", "fix!: rename flag", "chore: deps"])
    assert notes.startswith("# 1.3.0")
    assert notes.index("Breaking changes") < notes.index("Features") < notes.index("Fixes")
    assert "- add export" in notes and "- crash on empty" in notes and "- rename flag" in notes
    assert "deps" not in notes


def test_notes_for_empty_release_say_so():
    assert "No user-facing changes" in generate_notes("1.0.1", ["chore: deps"])
