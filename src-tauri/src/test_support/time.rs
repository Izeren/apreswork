// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Deterministic `DateTime` and `Instant` constructors for tests.

use std::sync::OnceLock;
use std::time::Instant;

use chrono::{DateTime, TimeZone, Utc};

/// Construct a fixed UTC `DateTime` (minute precision) for deterministic tests.
///
/// Seconds are always zero; the rare test that needs sub-minute precision adds
/// `+ Duration::seconds(n)` at the call site rather than burdening every caller
/// with a seconds argument.
#[must_use]
pub(crate) fn utc(year: i32, month: u32, day: u32, hour: u32, min: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(year, month, day, hour, min, 0)
        .unwrap()
}

/// Canonical "now" fixture for tests that mutate state and assert timestamps.
///
/// Chosen far ahead of any seeded chunk timestamp so `updated_at > old`
/// assertions hold without fragile comparisons against wall-clock time.
#[must_use]
pub(crate) fn test_now() -> DateTime<Utc> {
    utc(2030, 1, 1, 0, 0)
}

/// Canonical `Instant` for deterministic auth-deadline tests.
///
/// Initialized exactly once per process (the first test that calls this).
/// All tests reuse the same `Instant`. To simulate a point after `N` seconds,
/// use `test_instant_now() + Duration::from_secs(N)`.
#[must_use]
pub(crate) fn test_instant_now() -> Instant {
    static NOW: OnceLock<Instant> = OnceLock::new();
    *NOW.get_or_init(Instant::now)
}

/// Base instant for domain model test-default builders.
///
/// Mid-2026 so `base + 7 days` (the default task deadline) stays well before
/// `test_now()` (2030), matching the invariant that fixture deadlines are in the
/// past relative to mutation-test timestamps.
#[must_use]
pub(crate) fn fixture_base() -> DateTime<Utc> {
    utc(2026, 7, 1, 0, 0)
}
