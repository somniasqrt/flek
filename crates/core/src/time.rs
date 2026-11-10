//! Shared CLI time arguments, normalized without floating point.
use crate::{Error, Result};
use chrono::{DateTime, Local, NaiveDate, TimeZone, Utc};

pub fn parse(input: &str) -> Result<String> {
    parse_at(input, Utc::now())
}
pub fn parse_at(input: &str, now: DateTime<Utc>) -> Result<String> {
    let bad = || {
        Error::Usage(format!("invalid time {input:?}; expected Slack ts, YYYY-MM-DD, RFC3339 or duration (30m/12h/7d)"))
    };
    if crate::references::valid_ts(input) {
        let (s, u) = input.split_once('.').unwrap();
        let seconds: i64 = s.parse().map_err(|_| bad())?;
        DateTime::from_timestamp(seconds, u.parse::<u32>().map_err(|_| bad())? * 1000)
            .ok_or_else(bad)?;
        return Ok(format!("{seconds:010}.{u}"));
    }
    let dt = if let Ok(dt) = DateTime::parse_from_rfc3339(input) {
        dt.with_timezone(&Utc)
    } else if let Ok(date) = NaiveDate::parse_from_str(input, "%Y-%m-%d") {
        if input.len() != 10 || date.format("%Y-%m-%d").to_string() != input {
            return Err(bad());
        }
        Local
            .from_local_datetime(&date.and_hms_opt(0, 0, 0).unwrap())
            .single()
            .ok_or_else(bad)?
            .with_timezone(&Utc)
    } else {
        let (n, unit) = input
            .split_at_checked(input.len().saturating_sub(1))
            .ok_or_else(bad)?;
        let multiplier = match unit {
            "m" => 60,
            "h" => 3600,
            "d" => 86400,
            _ => return Err(bad()),
        };
        if n.is_empty() || !n.bytes().all(|b| b.is_ascii_digit()) {
            return Err(bad());
        }
        let seconds = n
            .parse::<i64>()
            .ok()
            .filter(|n| *n >= 0)
            .and_then(|n| n.checked_mul(multiplier))
            .ok_or_else(bad)?;
        now.checked_sub_signed(chrono::Duration::try_seconds(seconds).ok_or_else(bad)?)
            .ok_or_else(bad)?
    };
    if dt.timestamp() < 0 || dt.timestamp_subsec_micros() >= 1_000_000 {
        return Err(bad());
    }
    Ok(format!(
        "{:010}.{:06}",
        dt.timestamp(),
        dt.timestamp_subsec_micros()
    ))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn formats() {
        let now = DateTime::from_timestamp(1700000000, 0).unwrap();
        assert_eq!(parse_at("30m", now).unwrap(), "1699998200.000000");
        assert_eq!(
            parse("2023-11-14T22:13:20.000100Z").unwrap(),
            "1700000000.000100"
        );
        assert_eq!(parse("1700000000.000100").unwrap(), "1700000000.000100");
        let day = parse("2024-02-29").unwrap();
        let seconds: i64 = day.split('.').next().unwrap().parse().unwrap();
        assert_eq!(
            DateTime::from_timestamp(seconds, 0)
                .unwrap()
                .with_timezone(&Local)
                .format("%Y-%m-%d %H:%M:%S")
                .to_string(),
            "2024-02-29 00:00:00"
        );
        assert_eq!(parse_at("12h", now).unwrap(), "1699956800.000000");
        assert_eq!(parse_at("7d", now).unwrap(), "1699395200.000000");
        for bad in [
            "",
            "xyz",
            "2023-02-29",
            "-2h",
            "99999999999999999999d",
            "1.2",
            "2024-2-3",
            "+1h",
            "2016-12-31T23:59:60Z",
            "🌞",
        ] {
            assert!(parse(bad).is_err(), "{bad}");
        }
    }
}
