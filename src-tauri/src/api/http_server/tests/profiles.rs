// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Tests for profile-related REST endpoints and router state plumbing.

use axum::http::StatusCode;
use serde_json::json;
use test_case::test_case;

use super::{
    get_ok_array, get_request, memory_profiles_state, memory_state, profiles_state_with,
    TEST_PROFILE_ID, TEST_PROFILE_NAME,
};

/// Shared by all T2.4 switch tests to avoid repeating the
/// `build_router_with_profiles` + `json_request` call.
async fn post_switch(
    state: impl Into<crate::state::ActiveState>,
    profiles: std::sync::Arc<crate::profiles::ProfilesState>,
    body: serde_json::Value,
) -> axum::response::Response {
    let app = super::super::build_router_with_profiles(state, profiles, None);
    super::json_request(app, axum::http::Method::POST, "/api/profile/switch", body).await
}

#[test_case("/api/health" ; "health_endpoint")]
#[test_case("/api/tasks" ; "tasks_endpoint")]
#[tokio::test]
async fn build_router_with_profiles_serves_existing_routes(route: &str) {
    let state = memory_state();
    let profiles = memory_profiles_state();
    let app = super::super::build_router_with_profiles(state, profiles, None);
    let response = get_request(app, route).await;
    assert_eq!(response.status(), StatusCode::OK);
}

async fn list_profiles_json(
    profiles: std::sync::Arc<crate::profiles::ProfilesState>,
) -> Vec<serde_json::Value> {
    let app = super::super::build_router_with_profiles(memory_state(), profiles, None);
    get_ok_array(app, "/api/profiles").await
}

#[tokio::test]
async fn list_profiles_empty_registry_returns_empty_array() {
    assert!(list_profiles_json(memory_profiles_state()).await.is_empty());
}

#[tokio::test]
async fn list_profiles_returns_all_entries_with_id_name_created_at() {
    use crate::profiles::registry::test_support::entry;
    let json = list_profiles_json(profiles_state_with(vec![
        entry("p1", "Alice"),
        entry("p2", "Bob"),
    ]))
    .await;
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

#[test_case(json!({ "profile_id": "does-not-exist" }), StatusCode::NOT_FOUND, "not_found" ; "unknown_id_returns_404")]
#[test_case(json!({ "profile_id": "any", "expected_profile_id": "wrong-id" }), StatusCode::CONFLICT, "profile_mismatch" ; "expected_id_mismatch_returns_409")]
#[tokio::test]
async fn switch_profile_error_cases(
    body: serde_json::Value,
    expected_status: StatusCode,
    expected_error: &str,
) {
    let r = post_switch(memory_state(), memory_profiles_state(), body).await;
    assert_eq!(r.status(), expected_status);
    let json = super::body_json(r).await;
    assert_eq!(json["error"], expected_error);
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

#[tokio::test]
async fn switch_profile_same_id_is_noop() {
    let r = post_switch(
        memory_state(),
        memory_profiles_state(),
        json!({ "profile_id": TEST_PROFILE_ID }),
    )
    .await;
    assert_eq!(r.status(), StatusCode::OK);
    let json = super::body_json(r).await;
    assert_eq!(json["id"], TEST_PROFILE_ID);
    assert_eq!(json["name"], TEST_PROFILE_NAME);
}

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
        None,
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
        creds: None,
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
