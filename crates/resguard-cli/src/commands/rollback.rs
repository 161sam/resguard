use crate::*;

pub(crate) fn run(root: &str, state_dir: &str, last: bool, to: Option<String>) -> Result<i32> {
    handle_rollback(root, state_dir, last, to)
}
