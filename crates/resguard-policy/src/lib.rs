//! Policy engine crate for Resguard v3.
//!
//! Responsibility: validation and policy evaluation logic that turns
//! model/discovery inputs into resource-governance decisions.

pub mod autopilot;
pub mod autoprofile;
pub mod classification;
pub mod confidence;
pub mod rules;
pub mod thresholds;

pub use autopilot::{
    decide_autopilot_actions, AutopilotAction, AutopilotDecision, AutopilotPhase, AutopilotState,
    AutopilotTransition,
};
pub use autoprofile::{build_auto_profile, AutoProfileSnapshot};
pub use classification::{classify, ClassMatch, ClassificationInput};
pub use confidence::{score, strong_identity_match, ConfidenceScore, ConfidenceSignals};
pub use rules::default_suggest_rules;
pub use thresholds::{meets_confidence_threshold, validate_confidence_threshold};
