use crate::*;

pub(crate) fn run(root: &str, duration: Option<String>) -> Result<i32> {
    handle_panic(root, duration)
}
