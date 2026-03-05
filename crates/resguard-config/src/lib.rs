use anyhow::{anyhow, Result};
use resguard_core::{validate_profile, Profile, ValidationError};
use std::fs;
use std::path::{Path, PathBuf};

pub fn profiles_dir(config_dir: impl AsRef<Path>) -> PathBuf {
    config_dir.as_ref().join("profiles")
}

pub fn profile_path(config_dir: impl AsRef<Path>, name: &str) -> Result<PathBuf> {
    if name.trim().is_empty() {
        return Err(anyhow!("profile name must not be empty"));
    }
    if name.contains('/') || name.contains("..") {
        return Err(anyhow!(
            "invalid profile name: path separators are not allowed"
        ));
    }
    Ok(profiles_dir(config_dir).join(format!("{name}.yml")))
}

pub fn load_profile(path: impl AsRef<Path>) -> Result<Profile> {
    let content = fs::read_to_string(path)?;
    let profile: Profile = serde_yaml::from_str(&content)?;
    Ok(profile)
}

pub fn save_profile(path: impl AsRef<Path>, profile: &Profile) -> Result<()> {
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let content = serde_yaml::to_string(profile)?;
    fs::write(path, content)?;
    Ok(())
}

pub fn load_profile_from_store(config_dir: impl AsRef<Path>, name: &str) -> Result<Profile> {
    let path = profile_path(config_dir, name)?;
    load_profile(path)
}

pub fn save_profile_to_store(config_dir: impl AsRef<Path>, profile: &Profile) -> Result<PathBuf> {
    let path = profile_path(config_dir, &profile.metadata.name)?;
    save_profile(&path, profile)?;
    Ok(path)
}

pub fn list_profiles(config_dir: impl AsRef<Path>) -> Result<Vec<String>> {
    let mut names = Vec::new();
    let dir = profiles_dir(config_dir);

    if !dir.exists() {
        return Ok(names);
    }

    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        let Some(ext) = path.extension().and_then(|ext| ext.to_str()) else {
            continue;
        };

        if ext != "yml" && ext != "yaml" {
            continue;
        }

        if let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) {
            names.push(stem.to_string());
        }
    }

    names.sort();
    Ok(names)
}

pub fn validate_profile_file(path: impl AsRef<Path>) -> Result<Vec<ValidationError>> {
    let profile = load_profile(path)?;
    Ok(validate_profile(&profile))
}
