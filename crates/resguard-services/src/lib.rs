//! Services orchestration crate for Resguard v3.
//!
//! Responsibility: application services/use-cases that compose model,
//! policy, discovery, runtime, and config into higher-level workflows.

pub mod apply_service;
pub mod daemon_service;
pub mod desktop_service;
pub mod doctor_service;
pub mod metrics_service;
pub mod panic_service;
pub mod rescue_service;
pub mod setup_service;
pub mod suggest_service;
