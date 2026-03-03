// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Domain enums used throughout the application.

use serde::{Deserialize, Serialize};

/// Task priority level (higher numeric value = higher priority).
///
/// Uses `#[repr(u8)]` so numeric ordering matches variant ordering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum Priority {
    Low = 0,
    Medium = 1,
    High = 2,
    Critical = 3,
}

/// Lifecycle status of a [`super::models::Task`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskStatus {
    Backlog,
    Pending,
    Scheduled,
    Completed,
    Cancelled,
}

/// Status of a scheduled [`super::models::Chunk`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChunkStatus {
    Scheduled,
    Completed,
}
