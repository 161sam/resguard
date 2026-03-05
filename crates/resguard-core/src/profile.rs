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
    pub classes: BTreeMap<String, Class>,
    pub slices: Option<Slices>,
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
pub struct Class {
    pub slice_name: Option<String>,
    pub memory_high: Option<String>,
    pub memory_max: Option<String>,
    pub cpu_weight: Option<u16>,
    pub oomd_memory_pressure: Option<String>,
    pub oomd_memory_pressure_limit: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Slices {
    #[serde(default)]
    pub classes: BTreeMap<String, Class>,
}
