use anyhow::{anyhow, Result};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::{Command, ExitStatus};

pub fn daemon_reload() -> Result<()> {
    let status = Command::new("systemctl").arg("daemon-reload").status()?;
    if status.success() {
        Ok(())
    } else {
        Err(anyhow!(
            "systemctl daemon-reload failed with status {status}"
        ))
    }
}

pub fn daemon_reload_if_root(root: &str) -> Result<()> {
    if root == "/" {
        daemon_reload()?;
    }
    Ok(())
}

pub fn write_file(path: &Path, content: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, content)?;
    Ok(())
}

pub fn ensure_dir(path: &Path) -> Result<()> {
    fs::create_dir_all(path)?;
    Ok(())
}

pub fn exec_command(
    program: &str,
    args: &[String],
    envs: &BTreeMap<String, String>,
) -> Result<ExitStatus> {
    let status = Command::new(program).args(args).envs(envs).status()?;
    Ok(status)
}

pub fn check_command_success(program: &str, args: &[&str]) -> bool {
    Command::new(program)
        .args(args)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub fn systemctl_cat_unit(user: bool, unit: &str) -> Result<bool> {
    let mut cmd = Command::new("systemctl");
    if user {
        cmd.arg("--user");
    }
    let status = cmd.arg("cat").arg(unit).status()?;
    Ok(status.success())
}

pub fn systemd_run(user: bool, slice: &str, wait: bool, cmd: &[String]) -> Result<i32> {
    let mut c = Command::new("systemd-run");
    if user {
        c.arg("--user");
    }
    c.arg("--scope");
    if wait {
        c.arg("--wait");
    }
    c.arg("-p").arg(format!("Slice={slice}"));
    c.arg("--");
    c.args(cmd);

    let status = c.status()?;
    Ok(status.code().unwrap_or(1))
}

pub fn systemctl_show_props(
    user: bool,
    unit: &str,
    keys: &[&str],
) -> Result<BTreeMap<String, String>> {
    let mut cmd = Command::new("systemctl");
    if user {
        cmd.arg("--user");
    }

    cmd.arg("show").arg(unit);
    for key in keys {
        cmd.arg("-p").arg(key);
    }

    let out = cmd.output()?;
    if !out.status.success() {
        return Err(anyhow!(
            "systemctl show failed for {} (user={}): status={}",
            unit,
            user,
            out.status
        ));
    }

    let mut map = BTreeMap::new();
    let text = String::from_utf8_lossy(&out.stdout);
    for line in text.lines() {
        if let Some((k, v)) = line.split_once('=') {
            map.insert(k.trim().to_string(), v.trim().to_string());
        }
    }
    Ok(map)
}

pub fn systemctl_is_active(unit: &str) -> Result<bool> {
    let status = Command::new("systemctl")
        .arg("is-active")
        .arg(unit)
        .status()?;
    Ok(status.success())
}

pub fn systemctl_list_units(user: bool, unit_type: &str) -> Result<Vec<String>> {
    let mut cmd = Command::new("systemctl");
    if user {
        cmd.arg("--user");
    }
    let out = cmd
        .args([
            "list-units",
            "--type",
            unit_type,
            "--all",
            "--no-legend",
            "--no-pager",
        ])
        .output()?;

    if !out.status.success() {
        return Err(anyhow!(
            "systemctl list-units failed (user={}, type={}): {}",
            user,
            unit_type,
            out.status
        ));
    }

    let mut out_units = Vec::new();
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        if let Some(unit) = line.split_whitespace().next() {
            if !unit.is_empty() {
                out_units.push(unit.to_string());
            }
        }
    }
    Ok(out_units)
}

pub fn systemctl_service_action(action: &str, service: &str) -> Result<bool> {
    let status = Command::new("systemctl")
        .arg(action)
        .arg(service)
        .status()?;
    Ok(status.success())
}

pub fn systemctl_set_slice_memory_limits(
    slice: &str,
    memory_high: &str,
    memory_max: &str,
) -> Result<()> {
    systemctl_set_slice_limits(false, slice, Some(memory_high), Some(memory_max), None)
}

pub fn systemctl_set_slice_limits(
    user: bool,
    slice: &str,
    memory_high: Option<&str>,
    memory_max: Option<&str>,
    cpu_weight: Option<u16>,
) -> Result<()> {
    if memory_high.is_none() && memory_max.is_none() && cpu_weight.is_none() {
        return Err(anyhow!(
            "systemctl set-property called without any properties"
        ));
    }

    let mut cmd = Command::new("systemctl");
    if user {
        cmd.arg("--user");
    }

    cmd.arg("set-property").arg(slice);
    if let Some(v) = memory_high {
        cmd.arg(format!("MemoryHigh={v}"));
    }
    if let Some(v) = memory_max {
        cmd.arg(format!("MemoryMax={v}"));
    }
    if let Some(v) = cpu_weight {
        cmd.arg(format!("CPUWeight={v}"));
    }

    let status = cmd.status()?;
    if status.success() {
        Ok(())
    } else {
        Err(anyhow!(
            "systemctl set-property failed with status {status}"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::systemctl_set_slice_limits;

    #[test]
    fn set_slice_limits_requires_at_least_one_property() {
        let err = systemctl_set_slice_limits(false, "user.slice", None, None, None)
            .expect_err("no properties should fail");
        assert!(err.to_string().contains("without any properties"));
    }
}

pub fn resolve_user_runtime_dir(user: &str) -> Option<String> {
    let loginctl_output = Command::new("loginctl")
        .arg("show-user")
        .arg(user)
        .arg("-p")
        .arg("RuntimePath")
        .arg("--value")
        .output()
        .ok();

    if let Some(out) = loginctl_output {
        if out.status.success() {
            let v = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !v.is_empty() {
                return Some(v);
            }
        }
    }

    let id_output = Command::new("id").arg("-u").arg(user).output().ok()?;
    if !id_output.status.success() {
        return None;
    }
    let uid = String::from_utf8_lossy(&id_output.stdout)
        .trim()
        .to_string();
    if uid.is_empty() {
        return None;
    }
    Some(format!("/run/user/{uid}"))
}
