use anyhow::Result;

pub fn setup<FI, FA, FS>(
    profile_name: String,
    apply: bool,
    suggest: bool,
    mut init: FI,
    mut apply_fn: FA,
    mut suggest_fn: FS,
) -> Result<i32>
where
    FI: FnMut(&str) -> Result<i32>,
    FA: FnMut(&str) -> Result<i32>,
    FS: FnMut(&str) -> Result<i32>,
{
    println!("command=setup");
    println!(
        "profile={} apply={} suggest={}",
        profile_name, apply, suggest
    );

    let init_code = init(&profile_name)?;
    if init_code != 0 {
        return Ok(init_code);
    }

    let mut apply_code = 0;
    if apply {
        apply_code = apply_fn(&profile_name)?;
        if apply_code != 0 {
            println!("setup.result=partial");
            println!("setup.apply_exit={}", apply_code);
            println!("rollback_hint=resguard rollback --last");
            return Ok(apply_code);
        }
    }

    if suggest {
        println!("setup.suggest_preview=begin");
        let _ = suggest_fn(&profile_name)?;
        println!("setup.suggest_preview=end");
    }

    println!("setup.result=ok");
    println!("setup.profile={}", profile_name);
    println!("setup.apply_executed={}", apply);
    println!("changed=profile_initialized_and_apply_executed");
    println!("rollback_hint=resguard rollback --last");
    println!("user_reload_hint=systemctl --user daemon-reload");
    println!("followup=for desktop wraps run: resguard suggest --dry-run");
    println!("followup=apply suggestions with confidence gate: resguard suggest --apply");
    Ok(if apply { apply_code } else { 0 })
}
