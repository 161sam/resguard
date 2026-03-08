use crate::*;

pub(crate) fn run(root: &str, config_dir: &str, state_dir: &str, req: RunRequest) -> Result<i32> {
    handle_run(root, config_dir, state_dir, req)
}
