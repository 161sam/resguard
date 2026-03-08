use crate::cli::CompletionShell as CliCompletionShell;
use crate::*;

pub(crate) fn run() -> Result<i32> {
    print!("{}", cli_version_output());
    Ok(0)
}

pub(crate) fn completion(shell: CliCompletionShell) -> Result<i32> {
    let mapped = match shell {
        CliCompletionShell::Bash => CompletionShell::Bash,
        CliCompletionShell::Zsh => CompletionShell::Zsh,
        CliCompletionShell::Fish => CompletionShell::Fish,
    };
    handle_completion(mapped)
}
