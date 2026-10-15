//! Field types of the dot format, mirroring tod-cli's dot/types.

use anyhow::{bail, Result};
use chrono::{DateTime, Utc};

/// The format used by the original lib: "YYYY-MM-DD hh:mm:ss ZZ".
/// All dot dates are UTC, so the offset is written explicitly.
pub const ISO8601_FORMAT: &str = "%Y-%m-%d %H:%M:%S %z";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldType {
    /// Free text: a single line, or a block joined with newlines.
    Text,
    /// Boolean, stored as "1" or "0" (like the original lib).
    Bool,
    /// ISO8601 date, e.g. "2026-10-06 18:12:44 +0000".
    Iso8601,
}

/// Parse a boolean the way tod-cli did: only "1" is true.
pub fn parse_bool(value: &str) -> bool {
    value.trim() == "1"
}

pub fn stringify_bool(value: bool) -> &'static str {
    if value { "1" } else { "0" }
}

pub fn parse_datetime(value: &str) -> Result<DateTime<Utc>> {
    match DateTime::parse_from_str(value.trim(), ISO8601_FORMAT) {
        Ok(dt) => Ok(dt.with_timezone(&Utc)),
        Err(_) => bail!("invalid datetime: '{value}'"),
    }
}

pub fn stringify_datetime(value: &DateTime<Utc>) -> String {
    value.format(ISO8601_FORMAT).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bool_round_trip() {
        assert!(parse_bool("1"));
        assert!(!parse_bool("0"));
        assert!(!parse_bool("true"));
        assert_eq!(stringify_bool(true), "1");
        assert_eq!(stringify_bool(false), "0");
    }

    #[test]
    fn datetime_round_trip() {
        let dt = parse_datetime("2026-10-06 18:12:44 +0000").unwrap();
        assert_eq!(stringify_datetime(&dt), "2026-10-06 18:12:44 +0000");
    }

    #[test]
    fn invalid_datetime_is_rejected() {
        assert!(parse_datetime("not a date").is_err());
        assert!(parse_datetime("2026-10-06").is_err());
    }
}
