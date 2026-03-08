pub fn validate_confidence_threshold(threshold: u8) -> Result<(), String> {
    if threshold > 100 {
        Err("invalid --confidence-threshold: must be 0..=100".to_string())
    } else {
        Ok(())
    }
}

pub fn meets_confidence_threshold(score: u8, threshold: u8) -> bool {
    score >= threshold
}
