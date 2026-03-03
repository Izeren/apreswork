// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Tests for backup REST endpoints:
//! POST /api/backup/now, GET /api/backup/status.
//!
//! The fixture wires `NoopBackupTarget`, so these exercise the HTTP layer
//! (routing, status codes, JSON shape); the export/restore decision logic is
//! covered in `services::backup::tests` against a mock target.

use axum::http::{Method, StatusCode};
use serde_json::Value;

use crate::api::http_server::build_router;

use super::{body_json, memory_state, request};

#[tokio::test]
async fn backup_status_fresh_store_returns_defaults() {
    let app = build_router(memory_state());

    let response = request(app, Method::GET, "/api/backup/status").await;

    assert_eq!(response.status(), StatusCode::OK);
    let json = body_json(response).await;
    assert_eq!(json["enabled"], false);
    assert_eq!(json["connected"], false);
    assert_eq!(json["last_export_at"], Value::Null);
    assert_eq!(json["last_backup_error"], Value::Null);
    assert_eq!(json["restored_this_run"], Value::Null);
}

#[tokio::test]
async fn backup_now_without_a_connection_returns_400_validation() {
    let app = build_router(memory_state());

    let response = request(app, Method::POST, "/api/backup/now").await;

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let json = body_json(response).await;
    assert_eq!(json["error"], "validation");
}
