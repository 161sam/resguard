use crate::*;

pub(crate) fn handle_desktop_list(
    format: &str,
    filter: Option<String>,
    origin: DesktopOrigin,
) -> Result<i32> {
    println!("command=desktop list");

    let regex = if let Some(pat) = filter {
        Some(Regex::new(&pat).map_err(|err| anyhow!("invalid --filter regex: {}", err))?)
    } else {
        None
    };

    let items = discover_desktop_entries(origin, regex.as_ref())?;

    match format {
        "json" => println!("{}", serde_json::to_string_pretty(&items)?),
        "yaml" => println!("{}", serde_yaml::to_string(&items)?),
        _ => print_desktop_table(&items),
    }

    Ok(0)
}

pub(crate) fn handle_desktop_wrap(
    desktop_id: &str,
    class: &str,
    opts: DesktopWrapOptions,
) -> Result<i32> {
    let source = resolve_desktop_source(desktop_id)?;
    let wrapper_id = wrapper_desktop_id(&source.desktop_id, class);
    let target_path = if opts.override_mode {
        override_path_for(&source.desktop_id)?
    } else {
        wrapper_path_for(&source.desktop_id, class)?
    };
    let target_id = if opts.override_mode {
        source.desktop_id.clone()
    } else {
        wrapper_id.clone()
    };

    if opts.print_only && opts.dry_run {
        return Err(anyhow!(
            "invalid arguments: --print and --dry-run cannot be combined"
        ));
    }
    if opts.override_mode && !opts.force {
        return Err(anyhow!(
            "override mode is destructive by design: pass both --override and --force"
        ));
    }

    if target_path.exists() && !opts.force {
        return Err(anyhow!(
            "target already exists at {} (use --force to overwrite)",
            target_path.display()
        ));
    }

    if source
        .fields
        .get("DBusActivatable")
        .map(|v| v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
    {
        eprintln!("info: source desktop entry has DBusActivatable=true; wrapper will force DBusActivatable=false");
    }

    let wrapper_content = render_wrapper(&source.fields, class)?;
    if opts.print_only {
        print!("{wrapper_content}");
        return Ok(0);
    }

    if opts.dry_run {
        println!("command=desktop wrap");
        println!(
            "mode={}",
            if opts.override_mode {
                "override"
            } else {
                "wrapper"
            }
        );
        println!("desktop_id={}", source.desktop_id);
        println!("class={class}");
        println!("target_id={target_id}");
        println!("target_path={}", target_path.display());
        println!("write=false");
        println!(
            "{}",
            render_line_diff(
                &source.source_path.display().to_string(),
                &source.source_content,
                &target_path.display().to_string(),
                &wrapper_content
            )
        );
        return Ok(0);
    }

    if opts.override_mode {
        eprintln!("warn: --override writes directly to user desktop-id path");
        eprintln!("warn: target={}", target_path.display());
        eprintln!(
            "warn: backup will be stored in ~/.local/share/applications/.resguard-backup/<timestamp>/"
        );
    }

    if let Some(parent) = target_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    let mut backup_path: Option<PathBuf> = None;
    if opts.override_mode && target_path.exists() {
        backup_path = Some(create_override_backup(&target_path)?);
    }

    write_file(&target_path, &wrapper_content)
        .with_context(|| format!("failed to write wrapper {}", target_path.display()))?;

    let mut store = read_desktop_mapping_store()?;
    let by_class = store
        .mappings
        .entry(source.desktop_id.clone())
        .or_insert_with(BTreeMap::new);
    by_class.insert(
        class.to_string(),
        DesktopMappingEntry {
            wrapper_desktop_id: target_id.clone(),
            wrapper_path: target_path.display().to_string(),
            source_path: source.source_path.display().to_string(),
            created_at: now_timestamp_utc(),
            mode: Some(if opts.override_mode {
                "override".to_string()
            } else {
                "wrapper".to_string()
            }),
            backup_path: backup_path.as_ref().map(|p| p.display().to_string()),
        },
    );
    write_desktop_mapping_store(&store)?;

    println!("command=desktop wrap");
    println!("desktop_id={}", source.desktop_id);
    println!("class={class}");
    println!(
        "mode={}",
        if opts.override_mode {
            "override"
        } else {
            "wrapper"
        }
    );
    println!(
        "source={} ({})",
        source.source_path.display(),
        match source.origin {
            DesktopOrigin::User => "user",
            DesktopOrigin::System => "system",
            DesktopOrigin::All => "all",
        }
    );
    println!("wrapper_id={target_id}");
    println!("wrapper_path={}", target_path.display());
    if let Some(path) = backup_path {
        println!("backup_path={}", path.display());
    }
    println!("mapping_file={}", desktop_mapping_path()?.display());
    Ok(0)
}

pub(crate) fn handle_desktop_unwrap(
    desktop_id: &str,
    class: &str,
    opts: DesktopUnwrapOptions,
) -> Result<i32> {
    println!("command=desktop unwrap");
    let expected_wrapper_path = if opts.override_mode {
        override_path_for(desktop_id)?
    } else {
        wrapper_path_for(desktop_id, class)?
    };

    let mut store = read_desktop_mapping_store()?;
    let mut removed = false;
    let mut restored_backup_path: Option<PathBuf> = None;

    if let Some(by_class) = store.mappings.get_mut(desktop_id) {
        if let Some(entry) = by_class.remove(class) {
            if Path::new(&entry.wrapper_path) != expected_wrapper_path {
                eprintln!(
                    "warn: ignoring non-canonical wrapper path in mapping: {} (expected {})",
                    entry.wrapper_path,
                    expected_wrapper_path.display()
                );
            }
            if opts.override_mode {
                if let Some(backup) = &entry.backup_path {
                    let backup_path = PathBuf::from(backup);
                    if backup_path.is_file() {
                        if let Some(parent) = expected_wrapper_path.parent() {
                            fs::create_dir_all(parent).with_context(|| {
                                format!("failed to create {}", parent.display())
                            })?;
                        }
                        fs::copy(&backup_path, &expected_wrapper_path).with_context(|| {
                            format!(
                                "failed to restore backup {} to {}",
                                backup_path.display(),
                                expected_wrapper_path.display()
                            )
                        })?;
                        removed = true;
                        restored_backup_path = Some(backup_path);
                    }
                }
            }
            if !removed && expected_wrapper_path.exists() {
                fs::remove_file(&expected_wrapper_path).with_context(|| {
                    format!(
                        "failed to remove wrapper {}",
                        expected_wrapper_path.display()
                    )
                })?;
                removed = true;
            }
        }
        if by_class.is_empty() {
            store.mappings.remove(desktop_id);
        }
    }

    if !removed && expected_wrapper_path.exists() {
        fs::remove_file(&expected_wrapper_path).with_context(|| {
            format!(
                "failed to remove wrapper {}",
                expected_wrapper_path.display()
            )
        })?;
        removed = true;
    }

    write_desktop_mapping_store(&store)?;

    if removed {
        println!("desktop_id={desktop_id}");
        println!("class={class}");
        println!(
            "mode={}",
            if opts.override_mode {
                "override"
            } else {
                "wrapper"
            }
        );
        if let Some(path) = restored_backup_path {
            println!("status=restored-backup");
            println!("backup_path={}", path.display());
        } else {
            println!("status=removed");
        }
        Ok(0)
    } else {
        println!("desktop_id={desktop_id}");
        println!("class={class}");
        println!(
            "mode={}",
            if opts.override_mode {
                "override"
            } else {
                "wrapper"
            }
        );
        println!("status=not-found");
        Ok(1)
    }
}

pub(crate) fn handle_desktop_doctor() -> Result<i32> {
    let (partial, _) = run_desktop_doctor_checks(true, true)?;
    Ok(partial_exit_code(partial))
}
