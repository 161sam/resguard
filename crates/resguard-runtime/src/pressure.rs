use anyhow::Result;
use resguard_model::PressureSnapshot;

pub fn read_pressure(path: &str) -> Result<Option<PressureSnapshot>> {
    let s = std::fs::read_to_string(path)?;
    parse_pressure_snapshot(&s)
}

pub fn parse_pressure_snapshot(content: &str) -> Result<Option<PressureSnapshot>> {
    for line in content.lines() {
        if !line.starts_with("some ") {
            continue;
        }
        let mut avg10 = None;
        let mut avg60 = None;
        for tok in line.split_whitespace() {
            if let Some(v) = tok.strip_prefix("avg10=") {
                avg10 = Some(v.parse()?);
            } else if let Some(v) = tok.strip_prefix("avg60=") {
                avg60 = Some(v.parse()?);
            }
        }
        if let (Some(a10), Some(a60)) = (avg10, avg60) {
            return Ok(Some(PressureSnapshot {
                avg10: a10,
                avg60: a60,
            }));
        }
    }
    Ok(None)
}

pub fn read_pressure_1min(path: &str) -> Result<Option<f64>> {
    Ok(read_pressure(path)?.map(|p| p.avg60))
}

#[cfg(test)]
mod tests {
    use super::{parse_pressure_snapshot, read_pressure};
    use resguard_model::PressureSnapshot;
    use std::fs;
    use tempfile::NamedTempFile;

    #[test]
    fn parse_pressure_snapshot_from_some_line() {
        let file = NamedTempFile::new().expect("tmp");
        let content = "some avg10=1.23 avg60=4.56 avg300=0.00 total=1\nfull avg10=0.00 avg60=0.00 avg300=0.00 total=0\n";
        fs::write(file.path(), content).expect("write");
        let got = read_pressure(file.path().to_str().expect("path")).expect("read");
        assert_eq!(
            got,
            Some(PressureSnapshot {
                avg10: 1.23,
                avg60: 4.56
            })
        );
    }

    #[test]
    fn parse_from_buffer() {
        let content = "some avg10=2.00 avg60=3.00 avg300=1.00 total=10";
        let got = parse_pressure_snapshot(content).expect("parse");
        assert_eq!(
            got,
            Some(PressureSnapshot {
                avg10: 2.0,
                avg60: 3.0
            })
        );
    }
}
