// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Tests for profile-related REST endpoints and router state plumbing.

use axum::http::StatusCode;
use serde_json::json;

use super::{get_ok_array, get_request, memory_profiles_state, memory_state, profiles_state_with};

/// Shared by all T2.4 switch tests to avoid repeating the
/// `build_router_with_profiles` + `json_request` call.
async fn post_switch(
    state: impl Into<crate::state::ActiveState>,
    profiles: std::sync::Arc<crate::profiles::ProfilesState>,
    body: serde_json::Value,
) -> axum::response::Response {
    let app = super::super::build_router_with_profiles(state, profiles);
    super::json_request(app, axum::http::Method::POST, "/api/profile/switch", body).await
}

#[tokio::test]
async fn build_router_with_profiles_serves_existing_routes() {
    let state = memory_state();
    let profiles = memory_profiles_state();
    let app = super::super::build_router_with_profiles(state, profiles);

    let response = get_request(app.clone(), "/api/health").await;
    assert_eq!(response.status(), StatusCode::OK);

    let response = get_request(app, "/api/tasks").await;
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn list_profiles_empty_registry_returns_empty_array() {
    let state = memory_state();
    let profiles = memory_profiles_state();
    let app = super::super::build_router_with_profiles(state, profiles);
    let json = get_ok_array(app, "/api/profiles").await;
    assert!(json.is_empty());
}

/// Two-profile registry returns `200` with both entries in registration order.
/// Each entry carries `id`, `name`, and `created_at` (ISO 8601 string).
#[tokio::test]
async fn list_profiles_returns_all_entries_with_id_name_created_at() {
    use crate::profiles::registry::test_support::entry;
    let state = memory_state();
    let profiles = profiles_state_with(vec![entry("p1", "Alice"), entry("p2", "Bob")]);
    let app = super::super::build_router_with_profiles(state, profiles);
    let json = get_ok_array(app, "/api/profiles").await;
    assert_eq!(json.len(), 2);
    assert_eq!(json[0]["id"], "p1");
    assert_eq!(json[0]["name"], "Alice");
    assert!(
        json[0]["created_at"].is_string(),
        "created_at must be an ISO 8601 string"
    );
    assert_eq!(json[1]["id"], "p2");
    assert_eq!(json[1]["name"], "Bob");
}

#[tokio::test]
async fn switch_profile_unknown_id_returns_404() {
    let r = post_switch(
        memory_state(),
        memory_profiles_state(),
        json!({ "profile_id": "does-not-exist" }),
    )
    .await;
    super::assert_not_found(r).await;
}

#[tokio::test]
async fn switch_profile_expected_id_mismatch_returns_409() {
    let r = post_switch(
        memory_state(),
        memory_profiles_state(),
        json!({ "profile_id": "any", "expected_profile_id": "wrong-id" }),
    )
    .await;
    super::assert_guard_fired(r).await;
}

#[tokio::test]
async fn switch_profile_expected_id_with_no_active_profile_returns_400() {
    let r = post_switch(
        crate::state::ActiveState::new(),
        memory_profiles_state(),
        json!({ "profile_id": "any", "expected_profile_id": "some-id" }),
    )
    .await;
    super::assert_validation_error(r, "No profile").await;
}

/// Switching to the currently active profile is a no-op: returns 200 with the
/// current identity without executing the switch.
#[tokio::test]
async fn switch_profile_same_id_is_noop() {
    let r = post_switch(
        memory_state(),
        memory_profiles_state(),
        json!({ "profile_id": "test-profile-id" }),
    )
    .await;
    assert_eq!(r.status(), StatusCode::OK);
    let json = super::body_json(r).await;
    assert_eq!(json["id"], "test-profile-id");
    assert_eq!(json["name"], "Test Profile");
}

/// Happy path: switch from p-one to p-two updates the active slot.
/// Also exercises the `expected_profile_id` guard-passes branch.
#[tokio::test]
async fn switch_profile_happy_path_switches_slot() {
    use crate::profiles::registry::{test_support::entry, ProfilesRegistry, REGISTRY_VERSION};
    use crate::profiles::ProfilesState;
    use crate::state::ActiveState;
    use tempfile::tempdir;

    let dir = tempdir().expect("tempdir");
    let p_one = entry("p-one", "One");
    let p_two = entry("p-two", "Two");

    let active = ActiveState::new();
    crate::profiles::activate::switch_active_profile_direct(
        &active,
        dir.path(),
        &p_one,
        crate::test_support::test_now(),
    )
    .expect("activate p-one");

    let profiles = std::sync::Arc::new(ProfilesState::new(
        dir.path().to_path_buf(),
        ProfilesRegistry {
            version: REGISTRY_VERSION,
            last_used: None,
            profiles: vec![p_two],
        },
    ));
    let active_check = active.clone();
    let r = post_switch(
        active,
        profiles,
        json!({ "profile_id": "p-two", "expected_profile_id": "p-one" }),
    )
    .await;
    assert_eq!(r.status(), StatusCode::OK);
    let json = super::body_json(r).await;
    assert_eq!(json["id"], "p-two");
    assert_eq!(json["name"], "Two");
    assert_eq!(active_check.get().expect("switched").profile.id, "p-two");
}

#[test]
fn router_state_from_ref_projects_both_sub_states() {
    use axum::extract::FromRef;
    use std::sync::Arc;

    let state = memory_state();
    let profiles = memory_profiles_state();
    let router_state = super::super::RouterState {
        active: state.into(),
        profiles: profiles.clone(),
    };

    let extracted_active: crate::state::ActiveState = FromRef::from_ref(&router_state);
    assert!(
        extracted_active.get().is_ok(),
        "ActiveState projection works"
    );

    let extracted_profiles: Arc<crate::profiles::ProfilesState> = FromRef::from_ref(&router_state);
    assert!(
        Arc::ptr_eq(&extracted_profiles, &profiles),
        "Arc<ProfilesState> projection returns the same Arc"
    );
}
