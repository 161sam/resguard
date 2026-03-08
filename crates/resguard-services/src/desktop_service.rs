use anyhow::{anyhow, Result};
use regex::Regex;
use resguard_discovery::{discover_desktop_entries, DesktopEntry};
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopOrigin {
    User,
    System,
    All,
}

impl From<DesktopOrigin> for resguard_discovery::DesktopOrigin {
    fn from(value: DesktopOrigin) -> Self {
        match value {
            DesktopOrigin::User => resguard_discovery::DesktopOrigin::User,
            DesktopOrigin::System => resguard_discovery::DesktopOrigin::System,
            DesktopOrigin::All => resguard_discovery::DesktopOrigin::All,
        }
    }
}

pub fn desktop_list(format: &str, filter: Option<String>, origin: DesktopOrigin) -> Result<i32> {
    println!("command=desktop list");

    let regex = if let Some(pat) = filter {
        Some(Regex::new(&pat).map_err(|err| anyhow!("invalid --filter regex: {}", err))?)
    } else {
        None
    };

    let mut items = discover_desktop_entries(origin.into());
    if let Some(re) = &regex {
        items.retain(|item| {
            re.is_match(&item.desktop_id)
                || re.is_match(&item.name)
                || re.is_match(&item.exec)
                || re.is_match(&item.path)
        });
    }

    match format {
        "json" => println!(
            "{}",
            serde_json::to_string_pretty(
                &items.iter().map(DesktopListItem::from).collect::<Vec<_>>()
            )?
        ),
        "yaml" => println!(
            "{}",
            serde_yaml::to_string(&items.iter().map(DesktopListItem::from).collect::<Vec<_>>())?
        ),
        _ => print_desktop_table(&items),
    }

    Ok(0)
}

fn print_desktop_table(items: &[DesktopEntry]) {
    println!("desktop_id\torigin\tpath\tname\texec");
    for item in items {
        let origin = match item.origin {
            resguard_discovery::DesktopOrigin::User => "user",
            resguard_discovery::DesktopOrigin::System => "system",
            resguard_discovery::DesktopOrigin::All => "all",
        };
        println!(
            "{}\t{}\t{}\t{}\t{}",
            item.desktop_id,
            origin,
            item.path,
            item.name,
            item.exec
        );
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DesktopListItem {
    desktop_id: String,
    origin: String,
    path: String,
    name: String,
    exec: String,
}

impl From<&DesktopEntry> for DesktopListItem {
    fn from(value: &DesktopEntry) -> Self {
        let origin = match value.origin {
            resguard_discovery::DesktopOrigin::User => "user",
            resguard_discovery::DesktopOrigin::System => "system",
            resguard_discovery::DesktopOrigin::All => "all",
        }
        .to_string();
        Self {
            desktop_id: value.desktop_id.clone(),
            origin,
            path: value.path.clone(),
            name: value.name.clone(),
            exec: value.exec.clone(),
        }
    }
}

pub fn desktop_wrap<F>(f: F) -> Result<i32>
where
    F: FnOnce() -> Result<i32>,
{
    f()
}

pub fn desktop_unwrap<F>(f: F) -> Result<i32>
where
    F: FnOnce() -> Result<i32>,
{
    f()
}

pub fn desktop_doctor<F>(f: F) -> Result<i32>
where
    F: FnOnce() -> Result<i32>,
{
    f()
}
