// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

pub mod auth_commands;
pub mod backup_commands;
pub mod chunk_commands;
pub mod comment_commands;
pub mod config_commands;
pub mod profile_commands;
pub mod recurring_commands;
pub mod schedule_commands;
pub mod scheduler_commands;
pub mod task_commands;

/// Shared imports for command-layer Tauri mock-app test fixtures — a managed
/// `ActiveState`/`ProfilesState` on `tauri::test::mock_app()`. Used by
/// `backup_commands` and `profile_commands`, whose fixtures otherwise differ
/// (they manage different state types).
#[cfg(test)]
pub(crate) mod test_support {
    pub(crate) use std::sync::Arc;
    pub(crate) use tauri::test::MockRuntime;
    pub(crate) use tauri::Manager;
    pub(crate) use tempfile::{tempdir, TempDir};
}
