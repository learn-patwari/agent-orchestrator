---
name: semver-release
description: Suggest the next SemVer version from commits since the last tag. Use when cutting a release; the human confirms.
---
# SemVer release
1. List commit subjects since the last tag (`git log <tag>..HEAD --format=%B`).
2. `suggest_bump` (app/releases.py): breaking (`!` or `BREAKING CHANGE`) = major, `feat` = minor, else patch; no commits = no release.
3. `next_version(last_tag, bump)`; before 1.0.0 still follow the same rule.
4. Show the suggestion and the reason to the human; they may override. Never tag or push without the human's yes.
