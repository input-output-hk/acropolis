use std::time::Duration;

use anyhow::{bail, Result};

/// Suffix to nanoseconds multiplier (order matters: longer suffixes first)
const UNITS: &[(&str, f64)] = &[
    ("ns", 1.0),
    ("µs", 1_000.0),
    ("us", 1_000.0),
    ("ms", 1_000_000.0),
    ("s", 1_000_000_000.0),
];

/// Parse duration strings like "29.99s", "988.82ms", "16.958µs", "0ns"
pub fn parse_duration(s: &str) -> Result<Duration> {
    let s = s.trim();

    for (suffix, multiplier) in UNITS {
        if let Some(val_str) = s.strip_suffix(suffix) {
            let val: f64 = val_str.parse()?;
            return Ok(Duration::from_nanos((val * multiplier) as u64));
        }
    }

    bail!("Unknown duration format: {}", s)
}

/// Format a duration for display
pub fn format_duration(d: Duration) -> String {
    let nanos = d.as_nanos();
    if nanos == 0 {
        "0ns".to_string()
    } else if nanos < 1_000 {
        format!("{}ns", nanos)
    } else if nanos < 1_000_000 {
        format!("{:.2}µs", nanos as f64 / 1_000.0)
    } else if nanos < 1_000_000_000 {
        format!("{:.2}ms", nanos as f64 / 1_000_000.0)
    } else {
        format!("{:.2}s", d.as_secs_f64())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_seconds() {
        let d = parse_duration("29.992671083s").unwrap();
        assert!((d.as_secs_f64() - 29.992671083).abs() < 0.0001);
    }

    #[test]
    fn test_parse_milliseconds() {
        let d = parse_duration("988.82775ms").unwrap();
        assert!((d.as_secs_f64() - 0.98882775).abs() < 0.0001);
    }

    #[test]
    fn test_parse_microseconds() {
        let d = parse_duration("16.958µs").unwrap();
        assert_eq!(d.as_nanos(), 16958);
    }

    #[test]
    fn test_parse_nanoseconds() {
        let d = parse_duration("0ns").unwrap();
        assert_eq!(d.as_nanos(), 0);
    }
}
