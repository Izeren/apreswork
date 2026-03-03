// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

#![warn(clippy::pedantic)]
#![warn(clippy::cargo)]
#![deny(clippy::wildcard_imports)]
// Desktop app, not a published crate.
#![allow(clippy::cargo_common_metadata)]
// reqwest/oauth2 trees pin duplicate minor versions; tracked, not actionable here.
#![allow(clippy::multiple_crate_versions)]

use tauri::Manager as _;

pub mod api;
pub mod backup;
pub mod calendar;
pub mod commands;
pub mod db;
pub mod domain;
pub mod error;
pub mod profiles;
pub mod scheduler;
pub mod services;
pub mod state;
#[cfg(test)]
mod test_support;
pub mod traits;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
/// Initialize and run the Tauri application.
///
/// Resolves the app data/config directories, loads (or adopts) the profile
/// registry, and registers all Tauri commands. The store, timers, and REST
/// server start in `profiles::activate::activate_profile` — at startup for
/// the last-used profile; the frontend gate is only a fallback (activation
/// failure or an empty registry).
///
/// # Panics
///
/// Panics if the Tauri runtime fails to start (e.g., missing system
/// dependencies, invalid configuration, or display server unavailable).
#[allow(clippy::too_many_lines)] // length is the declarative command registration list, not logic
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            commands::task_commands::create_task,
            commands::task_commands::get_task,
            commands::task_commands::update_task,
            commands::task_commands::delete_task,
            commands::task_commands::cancel_task,
            commands::task_commands::complete_task,
            commands::task_commands::list_tasks,
            commands::task_commands::list_labels,
            commands::task_commands::get_orphaned_template_instances,
            commands::chunk_commands::list_chunks_for_task,
            commands::chunk_commands::list_chunks_in_range,
            commands::chunk_commands::get_agenda,
            commands::chunk_commands::list_external_events,
            commands::chunk_commands::create_fixed_chunk,
            commands::chunk_commands::complete_chunk,
            commands::chunk_commands::reopen_chunk,
            commands::chunk_commands::move_chunk,
            commands::chunk_commands::resize_chunk,
            commands::chunk_commands::lock_chunk,
            commands::chunk_commands::unlock_chunk,
            commands::chunk_commands::delete_fixed_chunk,
            commands::comment_commands::create_comment,
            commands::comment_commands::update_comment,
            commands::comment_commands::delete_comment,
            commands::comment_commands::list_comments,
            commands::schedule_commands::create_schedule,
            commands::schedule_commands::get_schedule,
            commands::schedule_commands::update_schedule,
            commands::schedule_commands::delete_schedule,
            commands::schedule_commands::list_schedules,
            commands::recurring_commands::create_template,
            commands::recurring_commands::get_template,
            commands::recurring_commands::update_template,
            commands::recurring_commands::delete_template,
            commands::recurring_commands::list_templates,
            commands::config_commands::get_config,
            commands::config_commands::update_config,
            commands::scheduler_commands::trigger_reschedule,
            commands::scheduler_commands::trigger_reschedule_incremental,
            commands::auth_commands::begin_google_auth,
            commands::auth_commands::google_auth_status,
            commands::auth_commands::google_disconnect,
            commands::auth_commands::google_list_calendars,
            commands::auth_commands::get_pull_calendars,
            commands::auth_commands::set_pull_calendars,
            commands::auth_commands::pull_external_events,
            commands::auth_commands::sync_now,
            commands::auth_commands::get_sync_status,
            commands::auth_commands::create_user_event,
            commands::auth_commands::update_user_event,
            commands::auth_commands::delete_user_event,
            commands::profile_commands::profile_status,
            commands::profile_commands::unlock_profile,
            commands::profile_commands::create_profile,
            commands::profile_commands::rename_profile,
            commands::profile_commands::delete_profile,
            commands::profile_commands::switch_profile,
            commands::backup_commands::get_backup_status,
            commands::backup_commands::set_backup_enabled,
            commands::backup_commands::backup_now,
            commands::backup_commands::export_backup_to_file,
            commands::backup_commands::import_backup_from_file,
        ])
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }

            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            let config_dir = app.path().app_config_dir()?;
            std::fs::create_dir_all(&config_dir)?;

            // Profiles (M13): load the registry, adopting pre-profiles data
            // on first run. Data-touching activation (DB open, restore check)
            // happens in activate_profile — only after a profile is picked
            // (profiles::activate module docs).
            let registry =
                profiles::adoption::load_or_adopt(&data_dir, &config_dir, chrono::Utc::now())?;
            let fast_path = registry.startup_profile().cloned();
            let profiles_state =
                std::sync::Arc::new(profiles::ProfilesState::new(data_dir.clone(), registry));
            app.manage(profiles_state.clone());

            // The active-profile slot plus the process-scoped workers that
            // resolve it per request/tick: the REST server and the trigger +
            // backup timers survive in-process profile switches by design.
            let active = state::ActiveState::new();
            app.manage(active.clone());
            {
                let slot = active.clone();
                services::trigger::start_background_timer(move || {
                    slot.get_opt().map(|s| s.trigger.clone())
                });
            }
            {
                let slot = active.clone();
                services::backup::start_backup_timer(
                    move || {
                        slot.get_opt().map(|s| services::backup::BackupContext {
                            store: s.store.clone(),
                            target: s.backup.clone(),
                            profile_dir: s.profile_dir.clone(),
                        })
                    },
                    services::backup::BACKUP_TIMER_POLL,
                );
            }
            let api_config = api::http_server::ServerConfig::from_env();
            tauri::async_runtime::spawn(async move {
                if let Err(e) =
                    api::http_server::start_server(active, profiles_state, api_config).await
                {
                    log::error!("Failed to start REST API server: {e}");
                }
            });

            if let Some(entry) = fast_path {
                // Auto-open the last-used profile — no picker, straight into
                // the data. Best-effort: on failure the slot stays empty and
                // the frontend falls back to the profile gate (retry there).
                if let Err(e) = profiles::activate::activate_profile(
                    app.handle(),
                    &data_dir,
                    &entry,
                    chrono::Utc::now(),
                ) {
                    log::error!(
                        "profiles: startup activation of '{}' failed: {e}",
                        entry.name
                    );
                }
            }

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while running tauri application")
        .run(|app_handle, event| {
            // Graceful-exit backup (Decision 5): a bounded final export so
            // close-laptop-A-open-laptop-B doesn't lose the last minutes.
            // `try_state`: setup manages the slot, but it may still be empty
            // (picker showing) — `get_opt` covers that.
            if let tauri::RunEvent::ExitRequested { .. } = event {
                let active = app_handle
                    .try_state::<state::ActiveState>()
                    .map(|s| s.inner().clone());
                if let Some(app_state) = active.and_then(|s| s.get_opt()) {
                    services::backup::export_on_exit(
                        app_state.store.clone(),
                        app_state.backup.clone(),
                        app_state.profile_dir.clone(),
                        std::time::Duration::from_secs(5),
                        chrono::Utc::now(),
                    );
                }
            }
        });
}
