//! The delivery pipeline (phase 5): worktrees, the dev/review flow, the merge gate and executor, GitHub access,
//! the release stages, the docs gate and the agent `pipeline` tool.
pub mod agent_tool;
pub mod docs_gate;
pub mod flow;
pub mod github;
pub mod merge_exec;
pub mod merge_gate;
pub mod release;
pub mod worktrees;
