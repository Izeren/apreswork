// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Concrete backup targets.
//!
//! Mirrors the `calendar` module layout: a real Google Drive target and a
//! no-op fallback, selected at the composition root alongside the calendar
//! provider (single selection policy in `calendar::providers_from_config`).

pub mod google_drive;
pub mod noop;
