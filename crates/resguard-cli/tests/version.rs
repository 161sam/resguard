use std::process::Command;

fn run_resguard(args: &[&str]) -> (bool, String, String) {
    let bin = env!("CARGO_BIN_EXE_resguard");
    let out = Command::new(bin)
        .args(args)
        .output()
        .expect("run resguard");
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn version_flag_matches_package_version() {
    let (ok, stdout, stderr) = run_resguard(&["--version"]);
    assert!(ok, "--version should exit successfully");
    assert!(stderr.is_empty(), "--version should not write stderr");
    assert!(
        stdout.contains(env!("CARGO_PKG_VERSION")),
        "expected version output to contain package version, got: {stdout:?}"
    );
}

#[test]
fn version_subcommand_matches_version_flag() {
    let (flag_ok, flag_stdout, flag_stderr) = run_resguard(&["--version"]);
    assert!(flag_ok, "--version should exit successfully");
    assert!(flag_stderr.is_empty(), "--version should not write stderr");

    let (cmd_ok, cmd_stdout, cmd_stderr) = run_resguard(&["version"]);
    assert!(cmd_ok, "version subcommand should exit successfully");
    assert!(cmd_stderr.is_empty(), "version should not write stderr");

    assert_eq!(
        cmd_stdout, flag_stdout,
        "version subcommand output must match --version output"
    );
}
