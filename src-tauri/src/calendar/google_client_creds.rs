// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Typed load/save methods for [`ClientCredentialStore`] and credential probe.
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

/// Google returns `"invalid_client"` when the credentials are wrong and any
/// other response (for example `"invalid_grant"` or `"redirect_uri_mismatch"`)
/// when the credentials are structurally valid.
///
/// The probe is best-effort: if Google is unreachable the credentials reach
/// the keyring and fail naturally on first use.
///
/// Google answers with `"invalid_grant"` or `"redirect_uri_mismatch"`, never
/// `"invalid_client"`, so the dummy redirect URI does not affect the outcome.
///
/// # Errors
///
/// Returns [`AppError::Validation`] only when Google returns
/// `"invalid_client"`.
pub(crate) fn probe_google_credentials(
    token_url: &str,
    client_id: &str,
    client_secret: &str,
) -> Result<(), AppError> {
    const PROBE_CODE: &str = "probe_only";
    const PROBE_REDIRECT: &str = "http://127.0.0.1:9999";

    let client = reqwest::blocking::Client::builder()
        // SSRF prevention: re-POST with body (incl. client_secret) on 307/308
        // without this guard.
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| {
            AppError::CalendarSync(format!("credential probe client build failed: {e}"))
        })?;

    let params = [
        ("grant_type", "authorization_code"),
        ("code", PROBE_CODE),
        ("client_id", client_id),
        ("client_secret", client_secret),
        ("redirect_uri", PROBE_REDIRECT),
    ];

    let Ok(resp) = client.post(token_url).form(&params).send() else {
        return Ok(()); // fail open: Google unreachable
    };

    let Ok(body) = resp.json::<serde_json::Value>() else {
        return Ok(()); // fail open: unexpected body format
    };

    if body.get("error").and_then(|v| v.as_str()) == Some("invalid_client") {
        return Err(AppError::Validation(
            "Google rejected the OAuth credentials. Check the client ID and secret.".into(),
        ));
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    use test_case::test_case;

    use super::probe_google_credentials;
    use crate::calendar::google_http::test_support::mock_server;
    use crate::error::AppError;

    const HTTP_OK: u16 = 200;
    const HTTP_BAD_REQUEST: u16 = 400;

    fn refused_url() -> String {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind listener");
        let port = listener.local_addr().expect("local addr").port();
        format!("http://127.0.0.1:{port}/token")
    }

    #[test]
    fn probe_rejects_invalid_client() {
        let (base, handle) = mock_server(vec![(
            HTTP_BAD_REQUEST,
            r#"{"error":"invalid_client"}"#.into(),
        )]);
        let url = format!("{base}/token");
        let result = probe_google_credentials(&url, "bad-id", "bad-secret");
        let requests = handle.join().expect("mock server panicked");
        assert!(
            matches!(result, Err(AppError::Validation(_))),
            "invalid_client must return Validation error; got: {result:?}"
        );
        let body = &requests[0].body;
        assert!(
            body.contains("grant_type=authorization_code"),
            "body: {body}"
        );
        assert!(body.contains("code=probe_only"), "body: {body}");
        assert!(body.contains("client_id=bad-id"), "body: {body}");
        assert!(body.contains("client_secret=bad-secret"), "body: {body}");
        assert!(body.contains("redirect_uri="), "body: {body}");
    }

    #[test_case(HTTP_BAD_REQUEST, r#"{"error":"invalid_grant"}"# ; "other_error")]
    #[test_case(HTTP_OK, r#"{"access_token":"tok"}"# ; "success_body")]
    #[test_case(HTTP_OK, "not json" ; "not_json_body")]
    fn probe_passes_on_non_invalid_client(status: u16, body: &str) {
        let (base, handle) = mock_server(vec![(status, body.into())]);
        let url = format!("{base}/token");
        let result = probe_google_credentials(&url, "id", "secret");
        handle.join().expect("mock server panicked");
        assert!(
            result.is_ok(),
            "non-invalid_client response must pass; got: {result:?}"
        );
    }

    #[test]
    fn probe_passes_when_request_fails() {
        let url = refused_url();
        let result = probe_google_credentials(&url, "id", "secret");
        assert!(
            result.is_ok(),
            "connection failure must pass (fail open); got: {result:?}"
        );
    }

    #[test]
    fn probe_error_message_omits_secret() {
        let (base, handle) = mock_server(vec![(
            HTTP_BAD_REQUEST,
            r#"{"error":"invalid_client"}"#.into(),
        )]);
        let url = format!("{base}/token");
        let secret = "GOCSPX-super-secret-value-12345";
        let err_msg = match probe_google_credentials(&url, "some-id", secret) {
            Err(AppError::Validation(msg)) => msg,
            other => panic!("expected Validation error, got: {other:?}"),
        };
        handle.join().expect("mock server panicked");
        assert!(
            !err_msg.contains(secret),
            "error message must not contain the client secret"
        );
    }
}
