pub const HEARTBEAT_MISS_GAP_MS: u64 = 1_500;

pub fn heartbeat_gap_miss(last_ms: Option<u64>, now_ms: Option<u64>) -> bool {
    match (last_ms, now_ms) {
        (Some(last), Some(now)) => now.saturating_sub(last) > HEARTBEAT_MISS_GAP_MS,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ignores_missing_clock_samples() {
        assert!(!heartbeat_gap_miss(None, Some(10)));
        assert!(!heartbeat_gap_miss(Some(10), None));
    }

    #[test]
    fn flags_only_real_gap_over_threshold() {
        assert!(!heartbeat_gap_miss(
            Some(100),
            Some(100 + HEARTBEAT_MISS_GAP_MS)
        ));
        assert!(heartbeat_gap_miss(
            Some(100),
            Some(101 + HEARTBEAT_MISS_GAP_MS)
        ));
    }
}
