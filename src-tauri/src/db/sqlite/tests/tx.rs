// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Tests for [`Store::with_tx`]: commit-on-Ok, rollback-on-Err, read-your-own-
//! writes inside the closure, and nested calls joining the outer transaction.
//!
//! NOTE: the connection mutex is non-reentrant — closures must only use the
//! `&dyn Store` they are handed, never the outer `SqliteStore` (self-deadlock).
//! That also applies to fixtures: build entities BEFORE entering `with_tx`.

use test_case::test_case;

use super::make_test_task;
use crate::db::sqlite::SqliteStore;
use crate::error::AppError;
use crate::traits::storage::{Store, TaskStore};

#[test]
fn with_tx_commits_on_ok() {
    let store = SqliteStore::new_in_memory();
    let task = make_test_task(&store);

    store
        .with_tx(&mut |tx| tx.create_task(&task))
        .expect("with_tx should commit");

    assert!(store.get_task(&task.id).expect("get_task").is_some());
}

#[test]
fn with_tx_rolls_back_on_err_and_propagates() {
    let store = SqliteStore::new_in_memory();
    let task = make_test_task(&store);

    let result = store.with_tx(&mut |tx| {
        tx.create_task(&task)?;
        Err(AppError::Database("boom".to_owned()))
    });

    let err = result.expect_err("closure error should propagate");
    assert!(
        matches!(err, AppError::Database(ref msg) if msg == "boom"),
        "expected the closure's own error, got: {err:?}"
    );
    assert!(store.get_task(&task.id).expect("get_task").is_none());
}

#[test]
fn with_tx_closure_reads_see_uncommitted_writes() {
    let store = SqliteStore::new_in_memory();
    let task = make_test_task(&store);

    store
        .with_tx(&mut |tx| {
            tx.create_task(&task)?;
            let loaded = tx.get_task(&task.id)?;
            assert!(loaded.is_some());
            Ok(())
        })
        .expect("with_tx");
}

#[test_case(false; "rolls_back_all")]
#[test_case(true; "commits_with_outer")]
fn nested_with_tx_behavior(should_commit: bool) {
    let store = SqliteStore::new_in_memory();
    let outer_task = make_test_task(&store);
    let inner_task = make_test_task(&store);

    let result = store.with_tx(&mut |tx| {
        tx.create_task(&outer_task)?;
        // Nested call joins the outer transaction instead of opening a new one.
        tx.with_tx(&mut |inner| inner.create_task(&inner_task))?;
        if should_commit {
            Ok(())
        } else {
            Err(AppError::Database("rollback both".to_owned()))
        }
    });

    if should_commit {
        result.expect("with_tx");
        assert!(store.get_task(&outer_task.id).expect("get outer").is_some());
        assert!(store.get_task(&inner_task.id).expect("get inner").is_some());
    } else {
        assert!(result.is_err());
        assert!(store.get_task(&outer_task.id).expect("get outer").is_none());
        assert!(store.get_task(&inner_task.id).expect("get inner").is_none());
    }
}
