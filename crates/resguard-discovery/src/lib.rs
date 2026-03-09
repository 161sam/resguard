//! Discovery crate for Resguard v3.
//!
//! Responsibility: workload and environment discovery (processes, desktop
//! integration signals, and host capabilities) in a reusable module.

pub mod alias;
pub mod desktop;
pub mod exec;
pub mod flatpak;
pub mod identity;
pub mod scope;
pub mod snap;
pub mod xdg;

pub use desktop::{
    discover_desktop_entries, resolve_desktop_id, scan_desktop_entries, DesktopEntry,
    ResolutionResult,
};
pub use exec::{parse_first_exec_token, parse_snap_run_app};
pub use flatpak::{
    flatpak_app_id_from_desktop_id, flatpak_app_name, parse_flatpak_app_from_scope,
    parse_flatpak_run_app,
};
pub use identity::{
    build_desktop_exec_index, parse_scope_identity, unique_desktop_id_for_scope_exec,
};
pub use scope::parse_snap_app_from_scope;
pub use snap::snap_app_from_desktop_id;
pub use xdg::{desktop_scan_dirs, DesktopOrigin};
