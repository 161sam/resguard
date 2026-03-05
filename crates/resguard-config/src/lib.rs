use std::fs;
use anyhow::Result;
use resguard_core::Profile;

pub fn load_profile(path: &str) -> Result<Profile> {

    let content = fs::read_to_string(path)?;

    let profile: Profile = serde_yaml::from_str(&content)?;

    Ok(profile)

}