use anyhow::Result;
use sqlx::postgres::types::PgInterval;
use std::time::Duration as StdDuration;
use time::Duration;

/// Extension trait for Duration conversions and formatting
/// Implemented for time::Duration, std::time::Duration, and PgInterval
pub trait DurationExt {
    /// Convert to time::Duration
    fn to_duration(&self) -> Result<Duration>;

    /// Convert to std::time::Duration
    fn to_std_duration(&self) -> Result<StdDuration>;

    /// Convert to PgInterval (PostgreSQL interval type)
    fn to_pg_interval(&self) -> Result<PgInterval>;

    /// Format duration in human-readable format (infallible - uses std::time::Duration)
    fn format_human(&self) -> String {
        // Convert to std duration and format, using 0 seconds as fallback for errors
        match self.to_std_duration() {
            Ok(std_duration) => humantime::format_duration(std_duration).to_string(),
            Err(_) => "0s".to_string(),
        }
    }
}

impl DurationExt for Duration {
    fn to_duration(&self) -> Result<Duration> {
        Ok(*self)
    }

    fn to_std_duration(&self) -> Result<StdDuration> {
        if self.is_negative() {
            anyhow::bail!(
                "Cannot convert negative duration ({}) to std::time::Duration",
                self
            );
        }
        Ok(StdDuration::from_secs(self.whole_seconds() as u64))
    }

    fn to_pg_interval(&self) -> Result<PgInterval> {
        let microseconds = self.whole_microseconds();
        if microseconds > i64::MAX as i128 {
            anyhow::bail!(
                "Duration too large to convert to PgInterval: {} microseconds",
                microseconds
            );
        }
        Ok(PgInterval {
            months: 0,
            days: 0,
            microseconds: microseconds as i64,
        })
    }
}

impl DurationExt for StdDuration {
    fn to_duration(&self) -> Result<Duration> {
        let seconds = self.as_secs();
        if seconds > i64::MAX as u64 {
            anyhow::bail!(
                "std::time::Duration too large to convert to time::Duration: {} seconds",
                seconds
            );
        }
        Ok(Duration::seconds(seconds as i64))
    }

    fn to_std_duration(&self) -> Result<StdDuration> {
        Ok(*self)
    }

    fn to_pg_interval(&self) -> Result<PgInterval> {
        let micros = self.as_micros();
        if micros > i64::MAX as u128 {
            anyhow::bail!(
                "std::time::Duration too large to convert to PgInterval: {} microseconds",
                micros
            );
        }
        Ok(PgInterval {
            months: 0,
            days: 0,
            microseconds: micros as i64,
        })
    }
}

impl DurationExt for PgInterval {
    fn to_duration(&self) -> Result<Duration> {
        if self.months != 0 || self.days != 0 {
            anyhow::bail!(
                "Cannot convert PgInterval with months ({}) or days ({}) to Duration - these cannot be precisely converted to microseconds",
                self.months,
                self.days
            );
        }
        Ok(Duration::microseconds(self.microseconds))
    }

    fn to_std_duration(&self) -> Result<StdDuration> {
        if self.months != 0 || self.days != 0 {
            anyhow::bail!(
                "Cannot convert PgInterval with months ({}) or days ({}) to std::time::Duration - these cannot be precisely converted to microseconds",
                self.months,
                self.days
            );
        }
        if self.microseconds < 0 {
            anyhow::bail!(
                "Cannot convert negative PgInterval ({} microseconds) to std::time::Duration",
                self.microseconds
            );
        }
        Ok(StdDuration::from_micros(self.microseconds as u64))
    }

    fn to_pg_interval(&self) -> Result<PgInterval> {
        Ok(*self)
    }
}
