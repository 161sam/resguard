use anyhow::Result;
use serde::Serialize;

pub fn render_json<T: Serialize>(value: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

pub fn render_yaml<T: Serialize>(value: &T) -> Result<()> {
    println!("{}", serde_yaml::to_string(value)?);
    Ok(())
}

pub fn render_table<S: AsRef<str>>(lines: &[S]) {
    for line in lines {
        println!("{}", line.as_ref());
    }
}

pub fn render_metrics(lines: &[String]) {
    render_table(lines);
}

pub fn render_suggestions(lines: &[String]) {
    render_table(lines);
}
