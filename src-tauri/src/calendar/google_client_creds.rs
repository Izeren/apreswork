// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Typed load/save methods for [`ClientCredentialStore`].
//!
//! Defined here — not in `google_token.rs` — to avoid a circular module
//! dependency: `google_token` does not import `google.rs`.

use serde::{Deserialize, Serialize};

use crate::calendar::google::GoogleCredentials;
use crate::calendar::google_token::{load_blob, save_blob, ClientCredentialStore};
use crate::error::AppError;

/// Private blob type for keyring serialization of [`GoogleCredentials`].
///
/// A private type prevents callers from serializing [`GoogleCredentials`]
/// directly, which reduces the risk of credential exposure in logs or IPC.
#[derive(Serialize, Deserialize)]
struct ClientCredBlob {
    client_id: String,
    client_secret: String,
}

impl ClientCredentialStore {
    /// Returns `Ok(None)` when no credential has been stored yet.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::CalendarSync`] when the keyring is unavailable or
    /// the stored blob cannot be deserialized.
    pub fn load(&self) -> Result<Option<GoogleCredentials>, AppError> {
        let blob: Option<ClientCredBlob> = load_blob(
            &self.entry,
            "stored client credentials are unreadable — re-enter them in Settings",
        )?;
        Ok(blob.map(|b| GoogleCredentials {
            client_id: b.client_id,
            client_secret: b.client_secret,
        }))
    }

    /// # Errors
    ///
    /// Returns [`AppError::CalendarSync`] when the keyring is unavailable.
    pub fn save(&self, data: &GoogleCredentials) -> Result<(), AppError> {
        save_blob(
            &self.entry,
            &ClientCredBlob {
                client_id: data.client_id.clone(),
                client_secret: data.client_secret.clone(),
            },
            "cannot serialize client credentials",
        )
    }
}
