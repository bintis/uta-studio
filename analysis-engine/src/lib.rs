//! Standalone, local Uta Analysis Engine contracts, planning, and execution boundary.
//!
//! This crate intentionally has no dependency on Uta! Studio's application core.

pub mod artifact;
pub mod audio;
pub mod candidate_pipeline;
pub mod cli;
pub mod conditional_scheduler;
pub mod contract;
pub mod engine;
pub mod execution;
pub mod fingerprint;
pub mod fusion;
pub mod planner;
pub mod quantization;
pub mod separation;
pub mod worker;
pub mod workflow;
pub mod workflow_executor;

pub use contract::*;
pub use engine::AnalysisEngine;
pub use planner::{EnginePlan, Planner};

pub const ENGINE_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const WORKER_PROTOCOL: &str = "uta.analysis-engine.worker";
pub const WORKER_PROTOCOL_VERSION: u32 = 1;
