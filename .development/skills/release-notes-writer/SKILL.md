---
name: release-notes-writer
description: Draft release notes for a version from commit subjects, for the human to edit. Use after the version is chosen.
---
# Release notes writer
1. `generate_notes(version, subjects)` gives a draft: Breaking changes, Features, Fixes; chores are left out.
2. Rewrite entries in plain user language; add upgrade steps under any breaking change.
3. Show the draft in the editor for the human to change; the approved text becomes the tag message and GitHub Release body.
4. Never invent changes that are not in the commits.
