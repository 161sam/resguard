use std::process::Command;
use anyhow::Result;

pub fn daemon_reload() -> Result<()> {

    Command::new("systemctl")
        .arg("daemon-reload")
        .status()?;

    Ok(())
}