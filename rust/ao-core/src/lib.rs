//! Agent Orchestrator core. Port of the Python backend, module by module (docs/design/rust-port.md).
pub mod auth;
pub mod config;
pub mod db;
pub mod handoffs;
pub mod inbox;
pub mod notify;
pub mod quota;
pub mod scheduler;
pub mod secrets;
