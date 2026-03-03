// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Assertions over service `Result`s — the `Validation` / `NotFound` error
//! shapes every service test checks. Centralised so the `matches!` boilerplate
//! lives in exactly one place instead of being copy-pasted per error-path test.

use std::fmt::Debug;

use crate::error::AppError;

/// Asserts `result` is an [`AppError::NotFound`] for the given `entity`/`id`.
pub(crate) fn assert_not_found<T: Debug>(result: &Result<T, AppError>, entity: &str, id: &str) {
    assert!(
        matches!(
            result,
            Err(AppError::NotFound { entity: e, id: i }) if e == entity && i == id
        ),
        "expected NotFound {{ {entity}, {id} }}, got: {result:?}"
    );
}

/// Asserts `result` is an [`AppError::Validation`] error (any message).
pub(crate) fn assert_validation<T: Debug>(result: &Result<T, AppError>) {
    assert!(
        matches!(result, Err(AppError::Validation(_))),
        "expected Validation error, got: {result:?}"
    );
}

/// Asserts `result` is an [`AppError::Validation`] whose message contains `needle`.
pub(crate) fn assert_validation_contains<T: Debug>(result: &Result<T, AppError>, needle: &str) {
    assert!(
        matches!(result, Err(AppError::Validation(msg)) if msg.contains(needle)),
        "expected Validation error containing {needle:?}, got: {result:?}"
    );
}
