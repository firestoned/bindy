// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Duration parsing for Go-style duration strings.
//!
//! Supports parsing duration strings in Go format (e.g., "720h", "30d", "4w") into
//! Rust `std::time::Duration`. Validates bounds according to RNDC key rotation requirements.

use anyhow::{bail, Context, Result};
use std::time::Duration;

use crate::constants::{MAX_ROTATION_INTERVAL_HOURS, MIN_ROTATION_INTERVAL_HOURS};

const SECONDS_PER_HOUR: u64 = 3600;
const SECONDS_PER_DAY: u64 = 86400;
const SECONDS_PER_WEEK: u64 = 604_800;

/// Parse a Go-style duration string into a Rust `Duration`.
///
/// Supported units:
/// - `h` (hours): "720h" = 30 days
/// - `d` (days): "30d" = 30 days
/// - `w` (weeks): "4w" = 28 days
///
/// # Constraints
///
/// - **Minimum:** 1 hour (`MIN_ROTATION_INTERVAL_HOURS`)
/// - **Maximum:** 8760 hours / 365 days (`MAX_ROTATION_INTERVAL_HOURS`)
///
/// These bounds ensure keys are rotated frequently enough for security compliance
/// but not so frequently as to cause operational issues.
///
/// # Examples
///
/// ```
/// use bindy::bind9::duration::parse_duration;
/// use std::time::Duration;
///
/// // Parse hours
/// assert_eq!(parse_duration("24h").unwrap(), Duration::from_secs(86400));
///
/// // Parse days
/// assert_eq!(parse_duration("30d").unwrap(), Duration::from_secs(2_592_000));
///
/// // Parse weeks
/// assert_eq!(parse_duration("4w").unwrap(), Duration::from_secs(2_419_200));
///
/// // Invalid formats return errors
/// assert!(parse_duration("").is_err());
/// assert!(parse_duration("10").is_err());  // Missing unit
/// assert!(parse_duration("10x").is_err()); // Invalid unit
/// ```
///
/// # Errors
///
/// Returns an error if:
/// - The format is invalid (missing unit, non-numeric value)
/// - The duration is below the minimum (1h)
/// - The duration is above the maximum (8760h / 365d / 52w)
///
/// # Panics
///
/// May panic if the `MIN_ROTATION_INTERVAL_HOURS` or `MAX_ROTATION_INTERVAL_HOURS`
/// constants overflow when multiplied by `SECONDS_PER_HOUR`. This should never
/// happen with the current constant values.
pub fn parse_duration(duration_str: &str) -> Result<Duration> {
    // Validate non-empty
    if duration_str.is_empty() {
        bail!("Duration string cannot be empty");
    }

    // Find where digits end and unit begins
    let split_pos = duration_str
        .chars()
        .position(|c| !c.is_ascii_digit())
        .context("Duration must end with a unit (h, d, or w)")?;

    // Split into value and unit
    let (value_str, unit) = duration_str.split_at(split_pos);

    // Parse numeric value
    let value: u64 = value_str
        .parse()
        .context("Duration value must be a positive integer")?;

    // Convert to seconds based on unit
    let seconds = match unit {
        "h" => value
            .checked_mul(SECONDS_PER_HOUR)
            .context("Duration value too large (overflow)")?,
        "d" => value
            .checked_mul(SECONDS_PER_DAY)
            .context("Duration value too large (overflow)")?,
        "w" => value
            .checked_mul(SECONDS_PER_WEEK)
            .context("Duration value too large (overflow)")?,
        _ => {
            bail!("Unsupported duration unit '{unit}'. Use 'h' (hours), 'd' (days), or 'w' (weeks)")
        }
    };

    // Validate bounds
    let min_seconds = MIN_ROTATION_INTERVAL_HOURS
        .checked_mul(SECONDS_PER_HOUR)
        .expect("MIN_ROTATION_INTERVAL_HOURS constant overflow");

    let max_seconds = MAX_ROTATION_INTERVAL_HOURS
        .checked_mul(SECONDS_PER_HOUR)
        .expect("MAX_ROTATION_INTERVAL_HOURS constant overflow");

    if seconds < min_seconds {
        bail!(
            "Duration '{duration_str}' is below minimum of {MIN_ROTATION_INTERVAL_HOURS}h (1 hour)"
        );
    }

    if seconds > max_seconds {
        bail!(
            "Duration '{duration_str}' exceeds maximum of {MAX_ROTATION_INTERVAL_HOURS}h (365 days)"
        );
    }

    Ok(Duration::from_secs(seconds))
}

#[cfg(test)]
#[path = "duration_tests.rs"]
mod duration_tests;
