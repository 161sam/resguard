//! Domain model crate for Resguard v3.
//!
//! Responsibility: shared data types and schema primitives used across
//! policy, discovery, runtime, services, CLI, and daemon layers.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    pub api_version: String,
    pub kind: String,
    pub metadata: Metadata,
    pub spec: Spec,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Metadata {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Spec {
    pub memory: Option<Memory>,
    pub cpu: Option<Cpu>,
    pub oomd: Option<Oomd>,
    #[serde(default)]
    pub classes: BTreeMap<String, ClassSpec>,
    pub slices: Option<Slices>,
    pub suggest: Option<Suggest>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Memory {
    pub system: Option<SystemMemory>,
    pub user: Option<UserMemory>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct SystemMemory {
    pub memory_low: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct UserMemory {
    pub memory_high: Option<String>,
    pub memory_max: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Cpu {
    pub enabled: Option<bool>,
    pub reserve_core_for_system: Option<bool>,
    pub system_allowed_cpus: Option<String>,
    pub user_allowed_cpus: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Oomd {
    pub enabled: Option<bool>,
    pub memory_pressure: Option<String>,
    pub memory_pressure_limit: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct ClassSpec {
    pub slice_name: Option<String>,
    pub memory_high: Option<String>,
    pub memory_max: Option<String>,
    pub cpu_weight: Option<u16>,
    pub oomd_memory_pressure: Option<String>,
    pub oomd_memory_pressure_limit: Option<String>,
}

pub type Class = ClassSpec;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Slices {
    #[serde(default)]
    pub classes: BTreeMap<String, ClassSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Suggest {
    #[serde(default)]
    pub rules: Vec<SuggestRule>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct SuggestRule {
    pub pattern: String,
    pub class: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum SuggestionReason {
    PatternRule,
    MemoryThreshold,
    StrongIdentity,
    DesktopIdMatch,
    Manual { message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct AppIdentity {
    pub executable: Option<String>,
    pub snap_app: Option<String>,
    pub desktop_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct DesktopEntryRef {
    pub desktop_id: String,
    pub origin: Option<String>,
    pub source: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Suggestion {
    pub scope: String,
    pub class: String,
    pub reason: SuggestionReason,
    pub slice: String,
    pub exec_start: String,
    pub memory_current: u64,
    pub cpu_usage_nsec: u64,
    pub desktop_id: Option<String>,
    pub confidence: u8,
    pub confidence_reason: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PressureSnapshot {
    pub avg10: f64,
    pub avg60: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct MetricsSnapshot {
    pub memory_pressure: Option<PressureSnapshot>,
    pub cpu_pressure: Option<PressureSnapshot>,
    pub io_pressure: Option<PressureSnapshot>,
    pub memory_current_bytes: Option<u64>,
    pub memory_available_bytes: Option<u64>,
    pub cpu_usage_nsec: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct ActionPlan {
    pub actions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct ApplyResult {
    pub success: bool,
    pub changed_paths: Vec<String>,
    pub backup_id: Option<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DoctorCheck {
    pub name: String,
    pub ok: bool,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct DoctorReport {
    pub checks: Vec<DoctorCheck>,
    pub partial: bool,
}
