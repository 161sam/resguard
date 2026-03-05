use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]

pub struct Profile {

    pub api_version: String,

    pub kind: String,

    pub metadata: Metadata,

    pub spec: Spec

}

#[derive(Debug, Serialize, Deserialize)]

pub struct Metadata {

    pub name: String

}

#[derive(Debug, Serialize, Deserialize)]

pub struct Spec {

    pub memory: Option<Memory>

}

#[derive(Debug, Serialize, Deserialize)]

pub struct Memory {

    pub system: Option<SystemMemory>,

    pub user: Option<UserMemory>

}

#[derive(Debug, Serialize, Deserialize)]

pub struct SystemMemory {

    pub memoryLow: Option<String>

}

#[derive(Debug, Serialize, Deserialize)]

pub struct UserMemory {

    pub memoryHigh: Option<String>,
    pub memoryMax: Option<String>

}