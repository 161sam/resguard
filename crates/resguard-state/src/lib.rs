use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct State {
    pub active_profile: Option<String>,
    pub backup_id: Option<String>,
    #[serde(default)]
    pub managed_paths: Vec<String>,
    #[serde(default)]
    pub created_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct BackupManifest {
    pub active_profile: Option<String>,
    pub backup_id: String,
    #[serde(default)]
    pub managed_paths: Vec<String>,
    #[serde(default)]
    pub created_paths: Vec<String>,
}

#[derive(Debug)]
pub struct ApplyTransaction {
    pub backup_id: String,
    pub backup_root: PathBuf,
    pub managed_paths: BTreeSet<PathBuf>,
    pub created_paths: BTreeSet<PathBuf>,
    backed_up_paths: BTreeSet<PathBuf>,
}

pub fn backup_path(backup_root: &Path, target: &Path, root: &Path) -> Result<PathBuf> {
    let rel = target.strip_prefix(root).unwrap_or(target);
    if rel.as_os_str().is_empty() {
        return Err(anyhow!(
            "invalid backup mapping for target {}",
            target.display()
        ));
    }
    Ok(backup_root.join(rel))
}

pub fn state_file_path(state_dir: &Path) -> PathBuf {
    state_dir.join("state.json")
}

pub fn backup_dir(state_dir: &Path, backup_id: &str) -> PathBuf {
    state_dir.join("backups").join(backup_id)
}

pub fn backup_manifest_path(state_dir: &Path, backup_id: &str) -> PathBuf {
    backup_dir(state_dir, backup_id).join("manifest.json")
}

pub fn begin_transaction(state_dir: &Path) -> Result<ApplyTransaction> {
    fs::create_dir_all(state_dir)?;

    let backup_id = current_backup_id()?;
    let backup_root = backup_dir(state_dir, &backup_id);
    fs::create_dir_all(&backup_root)?;

    Ok(ApplyTransaction {
        backup_id,
        backup_root,
        managed_paths: BTreeSet::new(),
        created_paths: BTreeSet::new(),
        backed_up_paths: BTreeSet::new(),
    })
}

pub fn snapshot_before_write(tx: &mut ApplyTransaction, target: &Path, root: &Path) -> Result<()> {
    tx.managed_paths.insert(target.to_path_buf());

    if tx.backed_up_paths.contains(target) || tx.created_paths.contains(target) {
        return Ok(());
    }

    if target.exists() {
        let dst = backup_path(&tx.backup_root, target, root)?;
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(target, &dst).with_context(|| {
            format!(
                "failed to backup '{}' to '{}'",
                target.display(),
                dst.display()
            )
        })?;
        tx.backed_up_paths.insert(target.to_path_buf());
    } else {
        tx.created_paths.insert(target.to_path_buf());
    }

    Ok(())
}

pub fn write_backup_manifest(state_dir: &Path, manifest: &BackupManifest) -> Result<()> {
    let path = backup_manifest_path(state_dir, &manifest.backup_id);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let content = serde_json::to_string_pretty(manifest)?;
    fs::write(path, content)?;
    Ok(())
}

pub fn read_backup_manifest(state_dir: &Path, backup_id: &str) -> Result<BackupManifest> {
    let path = backup_manifest_path(state_dir, backup_id);
    let content = fs::read_to_string(&path)
        .with_context(|| format!("failed to read backup manifest {}", path.display()))?;
    let manifest: BackupManifest = serde_json::from_str(&content)?;
    Ok(manifest)
}

pub fn write_state(state_dir: &Path, state: &State) -> Result<()> {
    fs::create_dir_all(state_dir)?;
    let path = state_file_path(state_dir);
    let content = serde_json::to_string_pretty(state)?;
    fs::write(path, content)?;
    Ok(())
}

pub fn read_state(state_dir: &Path) -> Result<State> {
    let path = state_file_path(state_dir);
    let content = fs::read_to_string(&path)
        .with_context(|| format!("failed to read state file {}", path.display()))?;
    let state: State = serde_json::from_str(&content)?;
    Ok(state)
}

pub fn manifest_from_transaction(
    tx: &ApplyTransaction,
    active_profile: Option<String>,
) -> BackupManifest {
    BackupManifest {
        active_profile,
        backup_id: tx.backup_id.clone(),
        managed_paths: tx
            .managed_paths
            .iter()
            .map(|path| path.to_string_lossy().to_string())
            .collect(),
        created_paths: tx
            .created_paths
            .iter()
            .map(|path| path.to_string_lossy().to_string())
            .collect(),
    }
}

pub fn state_from_manifest(manifest: &BackupManifest) -> State {
    State {
        active_profile: manifest.active_profile.clone(),
        backup_id: Some(manifest.backup_id.clone()),
        managed_paths: manifest.managed_paths.clone(),
        created_paths: manifest.created_paths.clone(),
    }
}

pub fn rollback_from_manifest(
    root: &Path,
    state_dir: &Path,
    manifest: &BackupManifest,
) -> Result<()> {
    let backup_root = backup_dir(state_dir, &manifest.backup_id);

    for path_str in &manifest.managed_paths {
        let target = PathBuf::from(path_str);
        let backup = backup_path(&backup_root, &target, root)?;
        if backup.exists() {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(&backup, &target).with_context(|| {
                format!(
                    "failed to restore '{}' from '{}'",
                    target.display(),
                    backup.display()
                )
            })?;
        }
    }

    for path_str in &manifest.created_paths {
        let path = PathBuf::from(path_str);
        if path.exists() {
            fs::remove_file(&path)
                .with_context(|| format!("failed to delete created path {}", path.display()))?;
        }
    }

    Ok(())
}

fn current_backup_id() -> Result<String> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock before UNIX_EPOCH")?;
    Ok(now.as_millis().to_string())
}
