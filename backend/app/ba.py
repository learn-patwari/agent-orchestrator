"""BA/Architect helpers: robust JSON parsing, lane planning, SRS assembly from the template (FR-33, FR-34).

Lane rule: tasks that touch a common file (directly or through a chain of tasks) are put in one lane and
stacked in plan order, so parallel developers never write the same file. `wave` is the dependency depth:
tasks in the same wave with different lanes can run in parallel.
"""
from __future__ import annotations

import json
import re


class BAError(Exception):
    pass


def parse_json(text: str) -> dict:
    t = text.strip()
    t = re.sub(r"^```(?:json)?\s*|\s*```$", "", t, flags=re.S)
    candidates = [t]
    if "{" in t and "}" in t:
        candidates.append(t[t.index("{"): t.rindex("}") + 1])
    for c in candidates:
        try:
            v = json.loads(c)
        except ValueError:
            continue
        if isinstance(v, dict):
            return v
    raise BAError("the model did not return valid JSON")


def lanes(plan: list[dict]) -> list[dict]:
    tasks = [t for ph in plan for t in ph["tasks"]]
    ids = [t["id"] for t in tasks]
    if len(set(ids)) != len(ids):
        raise BAError("duplicate task ids in plan")
    for t in tasks:
        for d in t.get("depends_on", []):
            if d not in ids:
                raise BAError(f"task {t['id']} depends on unknown task {d}")

    # waves = dependency depth (also detects cycles)
    depth: dict[str, int] = {}

    def wave(tid: str, seen: tuple = ()) -> int:
        if tid in seen:
            raise BAError(f"dependency cycle through {tid}")
        if tid not in depth:
            t = next(x for x in tasks if x["id"] == tid)
            depth[tid] = 1 + max([wave(d, seen + (tid,)) for d in t.get("depends_on", [])], default=-1)
        return depth[tid]

    for tid in ids:
        wave(tid)

    # union-find on shared files
    parent = {i: i for i in ids}

    def find(x: str) -> str:
        while parent[x] != x:
            parent[x] = parent[parent[x]]
            x = parent[x]
        return x

    owner: dict[str, str] = {}
    for t in tasks:
        for f in t.get("files", []):
            if f in owner:
                parent[find(t["id"])] = find(owner[f])
            else:
                owner[f] = t["id"]

    lane_name: dict[str, str] = {}
    last_in_lane: dict[str, str] = {}
    out = []
    for ph in plan:
        new_tasks = []
        for t in ph["tasks"]:
            root = find(t["id"])
            lane = lane_name.setdefault(root, f"L{len(lane_name) + 1}")
            new_tasks.append({**t, "lane": lane, "wave": depth[t["id"]], "stacked_on": last_in_lane.get(lane)})
            last_in_lane[lane] = t["id"]
        out.append({**ph, "tasks": new_tasks})
    return out


def build_srs(f: dict) -> str:
    a = f.get("analysis") or {}
    imp = a.get("impact") or {}
    decisions = [q for q in f.get("qa", []) if q["by"] == "human"]
    assumed = [q for q in f.get("qa", []) if q["by"] != "human"]
    lines = [f"# SRS: {f['title']}", "", "Status: Draft   Owner (human): ______   Date: ______", "",
             "> Written by the architect in BA/SRS mode. **D-n** is a decision you gave. **[A]** is a default "
             "chosen where you did not answer; you may overrule it.", "",
             "## 1. Purpose and background", a.get("restatement", f.get("text", "")), "", a.get("why", ""), "",
             "Original request: " + f.get("text", ""), "", "## 2. Decisions and defaults"]
    if not f.get("qa"):
        lines += ["No open decisions were raised."]
    else:
        lines += ["| ID | Decision |", "|---|---|"]
        lines += [f"| D-{i} | {q['question']}: {q['answer']} |" for i, q in enumerate(decisions, 1)]
        lines += [f"| A-{i} | {q['question']}: {q['answer']} **[A]** ({q['by'].replace('_', ' ')}) |"
                  for i, q in enumerate(assumed, 1)]
    lines += ["", "## 3. Impact on the existing system (architect view)",
              "Modules: " + ", ".join(imp.get("modules", [])) if imp.get("modules") else "Modules: none identified",
              "Data: " + ", ".join(imp.get("data", [])) if imp.get("data") else "Data: none identified",
              "Security: " + ", ".join(imp.get("security", [])) if imp.get("security") else "Security: none identified",
              "", "## 9. Risks", "| Risk | Impact | Mitigation |", "|---|---|---|"]
    lines += [f"| {r} | to assess | to be defined with the human |" for r in imp.get("risks", [])] or ["| none identified | | |"]
    lines += ["", "## 10. Development plan"]
    for ph in f.get("plan") or []:
        lines += ["", f"### {ph['id']}: {ph['name']}", "| Task | Files | Lane | Wave | Stacked on | Depends on |",
                  "|---|---|---|---|---|---|"]
        for t in ph["tasks"]:
            lines.append(f"| {t['id']}: {t.get('title', '')} | {', '.join(t.get('files', []))} | {t.get('lane', '')} | "
                         f"{t.get('wave', '')} | {t.get('stacked_on') or '-'} | {', '.join(t.get('depends_on', [])) or '-'} |")
    lines += ["", "## 11. Sign-off", "| Item | Value |", "|---|---|", "| Approved by | |", "| Date | |", ""]
    return "\n".join(lines)
