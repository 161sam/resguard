use crate::*;

pub(crate) fn handle_doctor(root: &str, state_dir: &str) -> Result<i32> {
    let has_desktop_mappings = read_desktop_mapping_store()
        .map(|s| !s.mappings.is_empty())
        .unwrap_or(false);

    resguard_services::doctor_service::doctor(root, state_dir, has_desktop_mappings, || {
        let (desktop_partial, _) = run_desktop_doctor_checks(false, false)?;
        Ok(desktop_partial)
    })
}
