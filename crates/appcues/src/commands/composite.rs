use std::time::{Duration, SystemTime};

const DAY: Duration = Duration::from_secs(86_400);

/// A half-open UTC window [start, end) for composite queries. Rendered as
/// RFC 3339 at second precision — the analytics API accepts that form for
/// start_time/end_time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Period {
    pub start: SystemTime,
    pub end: SystemTime,
}

impl Period {
    /// The `days`-long window ending at `now`.
    pub fn last_days(days: u32, now: SystemTime) -> Self {
        Period {
            start: now - DAY * days,
            end: now,
        }
    }

    /// The same-length window immediately before this one (deltas compare
    /// against it; it is derived, never a parameter — same rule as the
    /// analytics catalog).
    pub fn previous(&self) -> Self {
        let len = self
            .end
            .duration_since(self.start)
            .expect("period end is after start by construction");
        Period {
            start: self.start - len,
            end: self.start,
        }
    }

    pub fn start_str(&self) -> String {
        humantime::format_rfc3339_seconds(self.start).to_string()
    }

    pub fn end_str(&self) -> String {
        humantime::format_rfc3339_seconds(self.end).to_string()
    }
}

/// Percent change from prev to cur (50.0 means +50%), one decimal.
/// None when prev is 0 — a delta against nothing is not a number.
pub fn pct_change(cur: f64, prev: f64) -> Option<f64> {
    if prev == 0.0 {
        None
    } else {
        Some(round1((cur - prev) / prev * 100.0))
    }
}

/// part/whole as a percent (0–100), one decimal. None when whole is 0.
pub fn rate_pct(part: u64, whole: u64) -> Option<f64> {
    if whole == 0 {
        None
    } else {
        Some(round1(part as f64 / whole as f64 * 100.0))
    }
}

pub fn round1(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, SystemTime};

    fn t(s: &str) -> SystemTime {
        humantime::parse_rfc3339(s).unwrap()
    }

    #[test]
    fn last_days_window_ends_at_now() {
        let p = Period::last_days(7, t("2026-08-26T00:00:00Z"));
        assert_eq!(p.start_str(), "2026-08-19T00:00:00Z");
        assert_eq!(p.end_str(), "2026-08-26T00:00:00Z");
    }

    #[test]
    fn previous_period_is_adjacent_and_same_length() {
        let p = Period::last_days(7, t("2026-08-26T00:00:00Z"));
        let prev = p.previous();
        assert_eq!(prev.end_str(), p.start_str());
        assert_eq!(prev.start_str(), "2026-08-12T00:00:00Z");
        assert_eq!(
            prev.end.duration_since(prev.start).unwrap(),
            Duration::from_secs(7 * 86_400)
        );
    }

    #[test]
    fn previous_crosses_month_and_year_boundaries() {
        let p = Period::last_days(30, t("2026-01-15T12:00:00Z"));
        assert_eq!(p.start_str(), "2025-12-16T12:00:00Z");
        assert_eq!(p.previous().start_str(), "2025-11-16T12:00:00Z");
    }

    #[test]
    fn pct_change_handles_zero_previous() {
        assert_eq!(pct_change(150.0, 100.0), Some(50.0));
        assert_eq!(pct_change(80.0, 100.0), Some(-20.0));
        assert_eq!(pct_change(5.0, 0.0), None);
    }

    #[test]
    fn rate_pct_handles_zero_whole_and_rounds() {
        assert_eq!(rate_pct(1, 3), Some(33.3));
        assert_eq!(rate_pct(0, 10), Some(0.0));
        assert_eq!(rate_pct(3, 0), None);
    }
}
