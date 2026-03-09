//! Runtime backend crate for Resguard v3.
//!
//! Responsibility: systemd/cgroup/procfs integration, plan rendering,
//! plan execution, rollback helpers, and system snapshot collection.

pub mod adaptive;
pub mod cgroup;
pub mod executor;
pub mod meminfo;
pub mod planner;
pub mod pressure;
pub mod rollback;
pub mod snapshot;
pub mod systemd;

pub use adaptive::{
    apply_class_limit_changes, apply_class_limit_changes_with, plan_class_limit_changes,
    plan_class_limit_changes_with, read_class_limit_current, revert_class_limit_changes,
    revert_class_limit_changes_with, AdaptiveApplyResult, AdaptiveChangePlan, AdaptiveRevertPlan,
    AdaptiveRevertResult, ClassLimitCurrent, ClassLimitRequest, PlannedClassLimitChange,
};
pub use cgroup::{cpu_count, default_reserve_bytes, is_root_user, parse_prop_u64};
pub use executor::{execute_action, execute_plan, planned_write_changes, write_needs_change};
pub use meminfo::{
    parse_meminfo_field_kb, read_mem_available_bytes, read_mem_total_bytes, read_meminfo_kb,
};
pub use planner::{
    build_apply_plan, plan_apply, plan_apply_summary, render_class_slice,
    render_system_slice_dropin, render_user_slice_dropin, Action, PlanOptions,
};
pub use pressure::{parse_pressure_snapshot, read_pressure, read_pressure_1min};
pub use rollback::{rollback_apply_reload, rollback_slice_memory_limits};
pub use snapshot::{read_pressure_snapshot, read_system_snapshot, SystemSnapshot};
pub use systemd::{
    check_command_success, daemon_reload, daemon_reload_if_root, ensure_dir, exec_command,
    resolve_user_runtime_dir, systemctl_cat_unit, systemctl_is_active, systemctl_list_units,
    systemctl_service_action, systemctl_set_slice_limits, systemctl_set_slice_memory_limits,
    systemctl_show_props, systemd_run, write_file,
};
