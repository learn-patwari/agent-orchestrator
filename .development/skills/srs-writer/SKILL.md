---
name: srs-writer
description: Fills the SRS template from a feature, its decisions and its assumptions, with D-n decisions, A-n defaults marked [A], risks, a phased plan and an empty sign-off block. Use after intake questions are answered and before asking the human to approve.
---

# SRS writer

- Follow `templates/SRS.md` section by section; keep sections 1 (purpose), 2 (decisions and defaults), 3 (impact), 9 (risks), 10 (plan), 11 (sign-off).
- **D-n** = answered by the human. **A-n** = a default taken for them, always marked **[A]** with how it was taken (accepted default or auto default).
- Status is Draft until the human approves; the sign-off block stays empty for them to fill. Never mark it approved yourself.
- Explain the SRS in plain language to the human after writing it, in a few lines, and list what you assumed.
- Plan comes from `phased-plan-lanes`.
