use crate::planner::Action;
use crate::systemd::{ensure_dir, exec_command, write_file};
use anyhow::{anyhow, Result};
use resguard_model::ApplyResult;
use std::path::{Path, PathBuf};

pub fn execute_action(action: &Action) -> Result<()> {
    match action {
        Action::EnsureDir { path } => {
            ensure_dir(path)?;
            Ok(())
        }
        Action::WriteFile { path, content } => {
            write_file(path, content)?;
            Ok(())
        }
        Action::Exec {
            program,
            args,
            env,
            best_effort,
        } => match exec_command(program, args, env) {
            Ok(status) => {
                if status.success() {
                    Ok(())
                } else if *best_effort {
                    eprintln!(
                        "warn: best-effort command failed: {} {} (status={})",
                        program,
                        args.join(" "),
                        status
                    );
                    Ok(())
                } else {
                    Err(anyhow!(
                        "external command failed: {} {} (status={})",
                        program,
                        args.join(" "),
                        status
                    ))
                }
            }
            Err(err) => {
                if *best_effort {
                    eprintln!(
                        "warn: best-effort command failed to execute: {} {} ({})",
                        program,
                        args.join(" "),
                        err
                    );
                    Ok(())
                } else {
                    Err(err)
                }
            }
        },
    }
}

pub fn write_needs_change(path: &Path, desired: &str) -> Result<bool> {
    match std::fs::read_to_string(path) {
        Ok(current) => Ok(current != desired),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(true),
        Err(err)
            if err.kind() == std::io::ErrorKind::IsADirectory
                || err.kind() == std::io::ErrorKind::InvalidData =>
        {
            Ok(true)
        }
        Err(err) => Err(anyhow!("failed to read {}: {}", path.display(), err)),
    }
}

pub fn planned_write_changes(actions: &[Action]) -> Result<Vec<(PathBuf, String)>> {
    let mut out = Vec::new();
    for action in actions {
        if let Action::WriteFile { path, content } = action {
            if write_needs_change(path, content)? {
                out.push((path.clone(), content.clone()));
            }
        }
    }
    Ok(out)
}

pub fn execute_plan(plan: &[Action]) -> Result<ApplyResult> {
    let mut changed_paths = Vec::new();
    let mut warnings = Vec::new();

    for action in plan {
        match action {
            Action::WriteFile { path, .. } => changed_paths.push(path.display().to_string()),
            Action::Exec {
                program,
                args,
                best_effort,
                ..
            } if *best_effort => {
                warnings.push(format!(
                    "best-effort command may fail: {} {}",
                    program,
                    args.join(" ")
                ));
            }
            _ => {}
        }
        execute_action(action)?;
    }

    Ok(ApplyResult {
        success: true,
        changed_paths,
        backup_id: None,
        warnings,
    })
}
