//! Agent Orchestrator core. Port of the Python backend, module by module (docs/design/rust-port.md).
pub mod auth;
pub mod config;
pub mod db;
pub mod handoffs;
pub mod incidents;
pub mod inbox;
pub mod notify;
pub mod pipeline;
pub mod proc;
pub mod providers;
pub mod runtime;
pub mod quota;
pub mod releases;
pub mod rules;
pub mod scheduler;
pub mod secrets;
