use std::path::Path;

pub fn parse_first_exec_token(exec: &str) -> Option<String> {
    for tok in exec.split_whitespace() {
        if tok == "env" {
            continue;
        }
        if tok.contains('=') && tok.chars().next().is_some_and(|c| c.is_ascii_alphabetic()) {
            continue;
        }
        let cleaned = tok.trim_matches('"').trim_matches('\'');
        if cleaned.is_empty() {
            continue;
        }
        let base = Path::new(cleaned)
            .file_name()
            .and_then(|v| v.to_str())
            .unwrap_or(cleaned)
            .to_string();
        return Some(base);
    }
    None
}

pub fn parse_snap_run_app(exec: &str) -> Option<String> {
    let mut cleaned = Vec::new();
    for tok in exec.split_whitespace() {
        if tok == "env" {
            continue;
        }
        if tok.contains('=') && tok.chars().next().is_some_and(|c| c.is_ascii_alphabetic()) {
            continue;
        }
        let t = tok.trim_matches('"').trim_matches('\'');
        if !t.is_empty() {
            cleaned.push(t.to_string());
        }
    }

    let mut i = 0usize;
    while i + 2 < cleaned.len() {
        let base = Path::new(&cleaned[i])
            .file_name()
            .and_then(|v| v.to_str())
            .unwrap_or(&cleaned[i]);
        if base == "snap" && cleaned[i + 1] == "run" {
            for app in &cleaned[(i + 2)..] {
                if app.starts_with('-') {
                    continue;
                }
                return Some(app.to_string());
            }
            return None;
        }
        i += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{parse_first_exec_token, parse_snap_run_app};

    #[test]
    fn exec_token_parsing_works() {
        assert_eq!(
            parse_first_exec_token("env FOO=1 /usr/bin/firefox %u").as_deref(),
            Some("firefox")
        );
        assert_eq!(
            parse_snap_run_app("env BAMF=1 /usr/bin/snap run --command=sh code").as_deref(),
            Some("code")
        );
    }
}
