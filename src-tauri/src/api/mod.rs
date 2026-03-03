// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! REST API module.
//!
//! Contains the Axum HTTP server setup, router, and error mapping.
//! The server runs on `127.0.0.1` only (never `0.0.0.0`) on a configurable
//! port (default `19532`).

pub mod http_server;
