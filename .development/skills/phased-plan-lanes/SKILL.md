---
name: phased-plan-lanes
description: Splits a plan into small shippable phases and tasks, listing the files each task touches, and assigns lanes so parallel developers never write the same file. Use when turning an SRS into a development plan or when deciding what can run in parallel.
---

# Phased plan with lanes

- Phases are small and shippable (one PR each where possible). Each task lists the files it will touch and what it depends on.
- **Lane rule:** tasks that touch a common file, directly or through a chain of tasks, share a lane and are stacked in plan order (each PR based on the previous one, retargeted after the base merges). Different lanes can run in parallel.
- **Wave** = dependency depth: wave 0 starts first; a task waits for its dependencies.
- Reject plans with unknown dependencies or cycles.
- Keep the last step of a plan a verification step (full test run), not a build step.
