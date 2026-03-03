// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Google Drive backup target (visible file, `drive.file` scope).
//!
//! The backup lives at `Apreswork/apreswork-backup.zip` in the user's My
//! Drive. Freshness metadata rides in Drive `appProperties`, so the
//! stale-writer probe never downloads the archive. Every Drive `q` query is
//! built from compile-time constants only — no user input reaches it.
//!
//! Auth, 401-refresh and 403/429 backoff are shared with the Calendar client
//! via [`google_http::execute_authorized_raw`] (one retry policy definition).

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use chrono::{DateTime, Utc};

use crate::calendar::google::GoogleCalendarSync;
use crate::calendar::google_http::{execute_authorized_raw, BackoffPolicy};
use crate::error::AppError;
use crate::traits::backup::{BackupTarget, RemoteBackupMeta};
use crate::traits::calendar_sync::CalendarSync as _;

const BACKUP_FILE_NAME: &str = "apreswork-backup.zip";
const FOLDER_NAME: &str = "Apreswork";
const FOLDER_MIME: &str = "application/vnd.google-apps.folder";
/// Fixed multipart boundary: safe because both parts are app-generated
/// (JSON metadata and a zip archive; neither can contain this line).
const MULTIPART_BOUNDARY: &str = "apreswork-backup-boundary-2f6c1e";
const PROP_LAST_MUTATION: &str = "last_mutation";
const PROP_SCHEMA_VERSION: &str = "schema_version";
const PARSE_ERROR_MSG: &str = "Google Drive response parse error";

/// Drive REST endpoint URLs; injectable so tests never touch Google.
#[derive(Clone)]
pub struct DriveEndpoints {
    /// Files metadata endpoint (list / create metadata / download).
    pub files_url: String,
    /// Files upload endpoint (multipart create / update).
    pub upload_url: String,
}

impl Default for DriveEndpoints {
    fn default() -> Self {
        Self {
            files_url: "https://www.googleapis.com/drive/v3/files".to_owned(),
            upload_url: "https://www.googleapis.com/upload/drive/v3/files".to_owned(),
        }
    }
}

#[derive(serde::Deserialize)]
struct FileListResponse {
    #[serde(default)]
    files: Vec<FileItem>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct FileItem {
    id: Option<String>,
    app_properties: Option<HashMap<String, String>>,
}

#[derive(serde::Deserialize)]
struct FileCreateResponse {
    id: String,
}

/// Google Drive [`BackupTarget`], sharing auth plumbing with the Calendar
/// provider.
pub struct GoogleDriveBackup {
    sync: Arc<GoogleCalendarSync>,
    endpoints: DriveEndpoints,
    backoff: BackoffPolicy,
    /// Per-request timeout for metadata probes (startup-blocking → short).
    meta_timeout: Duration,
    /// Per-request timeout for archive transfers (upload/download).
    transfer_timeout: Duration,
    /// Drive file id of the backup, cached after the first lookup.
    cached_file_id: Mutex<Option<String>>,
}

impl GoogleDriveBackup {
    #[must_use]
    pub fn new(sync: Arc<GoogleCalendarSync>) -> Self {
        Self::with_options(
            sync,
            DriveEndpoints::default(),
            BackoffPolicy::default(),
            Duration::from_secs(3),
            Duration::from_secs(60),
        )
    }

    fn with_options(
        sync: Arc<GoogleCalendarSync>,
        endpoints: DriveEndpoints,
        backoff: BackoffPolicy,
        meta_timeout: Duration,
        transfer_timeout: Duration,
    ) -> Self {
        Self {
            sync,
            endpoints,
            backoff,
            meta_timeout,
            transfer_timeout,
            cached_file_id: Mutex::new(None),
        }
    }

    // A poisoned lock only means another thread panicked mid-access; the
    // cached Option is still valid, so recover instead of panicking.
    fn cache_file_id(&self, id: Option<String>) {
        *self
            .cached_file_id
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = id;
    }

    fn cached_file_id(&self) -> Option<String> {
        self.cached_file_id
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Authorized request returning the raw response (2xx not enforced).
    fn request_raw(
        &self,
        now: DateTime<Utc>,
        timeout: Duration,
        build: &dyn Fn(&str) -> reqwest::blocking::RequestBuilder,
    ) -> Result<reqwest::blocking::Response, AppError> {
        execute_authorized_raw(self.sync.as_ref(), &self.backoff, now, &|token| {
            build(token).timeout(timeout)
        })
        .map_err(to_backup_error)
    }

    /// Authorized request expecting a 2xx JSON body.
    fn request_json(
        &self,
        now: DateTime<Utc>,
        timeout: Duration,
        build: &dyn Fn(&str) -> reqwest::blocking::RequestBuilder,
    ) -> Result<serde_json::Value, AppError> {
        let response = self.request_raw(now, timeout, build)?;
        let status = response.status();
        if !status.is_success() {
            return Err(drive_http_error("request", status.as_u16()));
        }
        let text = response.text().map_err(|_| {
            AppError::Backup("Google Drive response parse error: failed to read body".into())
        })?;
        serde_json::from_str(&text).map_err(|e| backup_parse_err(&e))
    }

    /// List Drive files matching `q`, requesting only `fields` in the response.
    fn list_files(
        &self,
        now: DateTime<Utc>,
        q: &str,
        fields: &str,
    ) -> Result<FileListResponse, AppError> {
        let json = self.request_json(now, self.meta_timeout, &|token| {
            self.sync
                .http()
                .get(&self.endpoints.files_url)
                .bearer_auth(token)
                .query(&[("q", q), ("fields", fields), ("pageSize", "10")])
        })?;
        serde_json::from_value(json).map_err(|e| backup_parse_err(&e))
    }

    /// Build the shared multipart-upload request; caller picks method/url/body.
    fn multipart_request<'a>(
        &'a self,
        method: reqwest::Method,
        url: &'a str,
        body: &'a [u8],
    ) -> impl Fn(&str) -> reqwest::blocking::RequestBuilder + 'a {
        move |token: &str| {
            self.sync
                .http()
                .request(method.clone(), url)
                .bearer_auth(token)
                .query(&[("uploadType", "multipart")])
                .header(
                    reqwest::header::CONTENT_TYPE,
                    format!("multipart/related; boundary={MULTIPART_BOUNDARY}"),
                )
                .body(body.to_vec())
        }
    }

    fn resolve_file_id(&self, now: DateTime<Utc>) -> Result<Option<String>, AppError> {
        match self.cached_file_id() {
            Some(id) => Ok(Some(id)),
            None => Ok(self.find_backup_file(now)?.and_then(|f| f.id)),
        }
    }

    /// Look up the backup file by its fixed name; refreshes the id cache.
    fn find_backup_file(&self, now: DateTime<Utc>) -> Result<Option<FileItem>, AppError> {
        let q = format!("name = '{BACKUP_FILE_NAME}' and trashed = false");
        let resp = self.list_files(now, &q, "files(id,appProperties)")?;
        let found = resp.files.into_iter().find(|f| f.id.is_some());
        self.cache_file_id(found.as_ref().and_then(|f| f.id.clone()));
        Ok(found)
    }

    /// Find the `Apreswork` folder id, creating the folder when absent.
    fn ensure_folder_id(&self, now: DateTime<Utc>) -> Result<String, AppError> {
        let q =
            format!("name = '{FOLDER_NAME}' and mimeType = '{FOLDER_MIME}' and trashed = false");
        let resp = self.list_files(now, &q, "files(id)")?;
        if let Some(id) = resp.files.into_iter().find_map(|f| f.id) {
            return Ok(id);
        }

        let body = serde_json::json!({ "name": FOLDER_NAME, "mimeType": FOLDER_MIME });
        let json = self.request_json(now, self.meta_timeout, &|token| {
            self.sync
                .http()
                .post(&self.endpoints.files_url)
                .bearer_auth(token)
                .json(&body)
        })?;
        let created: FileCreateResponse =
            serde_json::from_value(json).map_err(|e| backup_parse_err(&e))?;
        Ok(created.id)
    }

    fn create_backup_file(
        &self,
        now: DateTime<Utc>,
        app_properties: &serde_json::Value,
        zip_bytes: &[u8],
    ) -> Result<(), AppError> {
        let folder_id = self.ensure_folder_id(now)?;
        let metadata = serde_json::json!({
            "name": BACKUP_FILE_NAME,
            "parents": [folder_id],
            "appProperties": app_properties,
        });
        let body = multipart_body(&metadata, zip_bytes);
        let build =
            self.multipart_request(reqwest::Method::POST, &self.endpoints.upload_url, &body);
        let json = self.request_json(now, self.transfer_timeout, &build)?;
        let created: FileCreateResponse =
            serde_json::from_value(json).map_err(|e| backup_parse_err(&e))?;
        self.cache_file_id(Some(created.id));
        Ok(())
    }
}

fn backup_parse_err(e: &serde_json::Error) -> AppError {
    AppError::Backup(format!("{PARSE_ERROR_MSG}: {e}"))
}

impl BackupTarget for GoogleDriveBackup {
    fn is_available(&self) -> bool {
        self.sync.is_available()
    }

    fn get_meta(&self, now: DateTime<Utc>) -> Result<Option<RemoteBackupMeta>, AppError> {
        let Some(file) = self.find_backup_file(now)? else {
            return Ok(None);
        };
        let props = file.app_properties.unwrap_or_default();
        let last_mutation = props
            .get(PROP_LAST_MUTATION)
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|d| d.with_timezone(&Utc));
        let schema_version = props
            .get(PROP_SCHEMA_VERSION)
            .and_then(|s| s.parse::<i64>().ok())
            // Informational only — the archive's own schema_version table is
            // the authoritative gate (extract_and_verify).
            .unwrap_or(0);
        Ok(Some(RemoteBackupMeta {
            last_mutation,
            schema_version,
        }))
    }

    fn upload(
        &self,
        now: DateTime<Utc>,
        zip_bytes: &[u8],
        meta: &RemoteBackupMeta,
    ) -> Result<(), AppError> {
        let app_properties = serde_json::json!({
            PROP_LAST_MUTATION: meta
                .last_mutation
                .map(|d| d.to_rfc3339())
                .unwrap_or_default(),
            PROP_SCHEMA_VERSION: meta.schema_version.to_string(),
        });

        let Some(id) = self.resolve_file_id(now)? else {
            return self.create_backup_file(now, &app_properties, zip_bytes);
        };

        let metadata = serde_json::json!({ "appProperties": app_properties });
        let body = multipart_body(&metadata, zip_bytes);
        let url = format!("{}/{id}", self.endpoints.upload_url);
        let build = self.multipart_request(reqwest::Method::PATCH, &url, &body);
        let response = self.request_raw(now, self.transfer_timeout, &build)?;

        let status = response.status();
        if status.as_u16() == 404 {
            // The visible file was deleted or moved by the user — recreate it.
            self.cache_file_id(None);
            return self.create_backup_file(now, &app_properties, zip_bytes);
        }
        if !status.is_success() {
            return Err(drive_http_error("upload", status.as_u16()));
        }
        Ok(())
    }

    fn download(&self, now: DateTime<Utc>) -> Result<Vec<u8>, AppError> {
        let Some(id) = self.resolve_file_id(now)? else {
            return Err(AppError::Backup("no backup file on Google Drive".into()));
        };

        let url = format!("{}/{id}", self.endpoints.files_url);
        let response = self.request_raw(now, self.transfer_timeout, &|token| {
            self.sync
                .http()
                .get(&url)
                .bearer_auth(token)
                .query(&[("alt", "media")])
        })?;
        let status = response.status();
        if !status.is_success() {
            return Err(drive_http_error("download", status.as_u16()));
        }
        let bytes = response.bytes().map_err(|_| {
            AppError::Backup("Google Drive download failed: could not read body".into())
        })?;
        Ok(bytes.to_vec())
    }
}

fn drive_http_error(context: &str, status: u16) -> AppError {
    AppError::Backup(format!("Google Drive {context} failed with HTTP {status}"))
}

/// Map shared-transport errors (phrased for calendar sync) into backup errors.
fn to_backup_error(err: AppError) -> AppError {
    match err {
        AppError::CalendarSync(msg) => AppError::Backup(msg),
        other => other,
    }
}

/// Build a `multipart/related` body: JSON metadata part + zip media part.
fn multipart_body(metadata: &serde_json::Value, zip_bytes: &[u8]) -> Vec<u8> {
    let head = format!(
        "--{MULTIPART_BOUNDARY}\r\n\
         Content-Type: application/json; charset=UTF-8\r\n\r\n\
         {metadata}\r\n\
         --{MULTIPART_BOUNDARY}\r\n\
         Content-Type: application/zip\r\n\r\n"
    );
    let tail = format!("\r\n--{MULTIPART_BOUNDARY}--\r\n");
    let mut body = Vec::with_capacity(head.len() + zip_bytes.len() + tail.len());
    body.extend_from_slice(head.as_bytes());
    body.extend_from_slice(zip_bytes);
    body.extend_from_slice(tail.as_bytes());
    body
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use chrono::{TimeZone as _, Utc};

    use super::{DriveEndpoints, GoogleDriveBackup};
    use crate::calendar::google::GoogleCalendarSync;
    use crate::calendar::google_http::test_support::{
        instant_backoff, mock_server, provider_with_token, test_creds, RecordedRequest,
    };
    use crate::error::AppError;
    use crate::test_support::test_now;
    use crate::traits::backup::{BackupTarget, RemoteBackupMeta};

    fn drive_with(base_url: &str) -> GoogleDriveBackup {
        let provider = provider_with_token(
            &format!("{base_url}/token"),
            &format!("{base_url}/calendar"),
            "test-access-token",
        );
        GoogleDriveBackup::with_options(
            Arc::new(provider),
            DriveEndpoints {
                files_url: format!("{base_url}/drive/v3/files"),
                upload_url: format!("{base_url}/upload/drive/v3/files"),
            },
            instant_backoff(),
            Duration::from_secs(5),
            Duration::from_secs(5),
        )
    }

    fn file_list(files: &str) -> (u16, String) {
        (200, format!("{{\"files\": [{files}]}}"))
    }

    /// Spin up a mock server and a [`GoogleDriveBackup`] pointed at it.
    fn mock_drive(
        responses: Vec<(u16, String)>,
    ) -> (
        GoogleDriveBackup,
        std::thread::JoinHandle<Vec<RecordedRequest>>,
    ) {
        let (base_url, handle) = mock_server(responses);
        (drive_with(&base_url), handle)
    }

    /// Mock-drive an `upload(FAKEZIP, ..)` call and return the recorded requests.
    fn upload_fakezip(responses: Vec<(u16, String)>) -> Vec<RecordedRequest> {
        let (drive, handle) = mock_drive(responses);
        drive
            .upload(test_now(), b"FAKEZIP", &meta_at_ten())
            .expect("upload");
        handle.join().expect("mock")
    }

    #[test]
    fn is_available_reflects_token_presence() {
        let connected = drive_with("http://127.0.0.1:1");
        assert!(connected.is_available(), "seeded token → available");

        // Empty keyring (no credential seeded) → not available.
        let empty_keyring = {
            use std::sync::Arc as StdArc;
            StdArc::new(keyring::Entry::new_with_credential(Box::new(
                keyring::mock::MockCredential::default(),
            )))
        };
        let disconnected = GoogleDriveBackup::new(Arc::new(GoogleCalendarSync::with_mock_keyring(
            test_creds(),
            crate::calendar::google_token::KeyringStore::with_mock_entry(empty_keyring),
            crate::calendar::google::GoogleEndpoints {
                auth_url: "http://127.0.0.1:1/auth".to_owned(),
                token_url: "http://127.0.0.1:1/token".to_owned(),
                api_base_url: "http://127.0.0.1:1/calendar".to_owned(),
            },
            Duration::from_secs(1),
        )));
        assert!(!disconnected.is_available(), "no token → unavailable");
    }

    #[test]
    fn get_meta_parses_app_properties() {
        let (base_url, handle) = mock_server(vec![file_list(
            "{\"id\": \"f1\", \"appProperties\": \
             {\"last_mutation\": \"2026-07-12T10:00:00+00:00\", \"schema_version\": \"6\"}}",
        )]);
        let drive = drive_with(&base_url);

        let meta = drive
            .get_meta(test_now())
            .expect("meta")
            .expect("backup present");

        assert_eq!(
            meta.last_mutation,
            Some(
                Utc.with_ymd_and_hms(2026, 7, 12, 10, 0, 0)
                    .single()
                    .expect("time")
            )
        );
        assert_eq!(meta.schema_version, 6);
        let recorded = handle.join().expect("mock");
        assert_eq!(recorded[0].method, "GET");
        assert!(
            recorded[0].path_with_query.contains("apreswork-backup.zip"),
            "lookup queries the fixed name: {}",
            recorded[0].path_with_query
        );
    }

    #[test]
    fn get_meta_reads_blank_or_missing_properties_leniently() {
        let (base_url, handle) = mock_server(vec![file_list("{\"id\": \"f1\"}")]);
        let drive = drive_with(&base_url);

        let meta = drive
            .get_meta(test_now())
            .expect("meta")
            .expect("backup present");

        assert_eq!(meta.last_mutation, None);
        assert_eq!(meta.schema_version, 0);
        drop(handle.join().expect("mock"));
    }

    #[test]
    fn get_meta_returns_none_when_no_backup_exists() {
        let (base_url, handle) = mock_server(vec![file_list("")]);
        let drive = drive_with(&base_url);

        assert_eq!(drive.get_meta(test_now()).expect("meta"), None);
        drop(handle.join().expect("mock"));
    }

    #[test]
    fn get_meta_maps_http_errors_to_backup_errors() {
        let (base_url, handle) = mock_server(vec![(500, "{}".to_owned())]);
        let drive = drive_with(&base_url);

        let err = drive.get_meta(test_now()).expect_err("must fail");
        assert!(matches!(err, AppError::Backup(_)), "got: {err}");
        drop(handle.join().expect("mock"));
    }

    fn meta_at_ten() -> RemoteBackupMeta {
        RemoteBackupMeta {
            last_mutation: Some(
                Utc.with_ymd_and_hms(2026, 7, 12, 10, 0, 0)
                    .single()
                    .expect("time"),
            ),
            schema_version: 6,
        }
    }

    #[test]
    fn upload_updates_the_existing_file_via_multipart_patch() {
        let recorded = upload_fakezip(vec![
            file_list("{\"id\": \"f1\"}"),
            (200, "{\"id\": \"f1\"}".to_owned()),
        ]);
        let patch = &recorded[1];
        assert_eq!(patch.method, "PATCH");
        assert!(patch.path_with_query.contains("/upload/drive/v3/files/f1"));
        assert!(patch.path_with_query.contains("uploadType=multipart"));
        assert!(
            patch.body.contains("appProperties"),
            "metadata part present"
        );
        assert!(
            patch.body.contains("2026-07-12T10:00:00"),
            "last_mutation stamped"
        );
        assert!(
            patch.body.contains("\"schema_version\":\"6\""),
            "schema stamped"
        );
        assert!(patch.body.contains("FAKEZIP"), "media part present");
    }

    #[test]
    fn upload_creates_file_and_folder_when_absent() {
        let recorded = upload_fakezip(vec![
            file_list(""),                           // backup lookup: absent
            file_list(""),                           // folder lookup: absent
            (200, "{\"id\": \"fold1\"}".to_owned()), // folder create
            (200, "{\"id\": \"f2\"}".to_owned()),    // multipart create
        ]);
        let folder_create = &recorded[2];
        assert_eq!(folder_create.method, "POST");
        assert!(folder_create
            .body
            .contains("application/vnd.google-apps.folder"));
        let create = &recorded[3];
        assert_eq!(create.method, "POST");
        assert!(create.path_with_query.contains("/upload/drive/v3/files"));
        assert!(create.path_with_query.contains("uploadType=multipart"));
        assert!(create.body.contains("apreswork-backup.zip"), "named file");
        assert!(create.body.contains("fold1"), "parent folder set");
        assert!(create.body.contains("FAKEZIP"), "media part present");
    }

    #[test]
    fn upload_recreates_the_file_when_the_old_one_vanished() {
        let recorded = upload_fakezip(vec![
            file_list("{\"id\": \"f1\"}"),        // backup lookup: stale id
            (404, "{}".to_owned()),               // PATCH: file gone
            file_list("{\"id\": \"fold1\"}"),     // folder lookup: exists
            (200, "{\"id\": \"f3\"}".to_owned()), // multipart create
        ]);
        assert_eq!(recorded[1].method, "PATCH");
        assert_eq!(recorded[3].method, "POST", "fell back to create once");
    }

    #[test]
    fn get_meta_caches_the_file_id_for_the_following_upload() {
        let (drive, handle) = mock_drive(vec![
            file_list("{\"id\": \"f1\"}"),
            (200, "{\"id\": \"f1\"}".to_owned()),
        ]);

        drive.get_meta(test_now()).expect("meta");
        drive
            .upload(test_now(), b"FAKEZIP", &meta_at_ten())
            .expect("upload");

        let recorded = handle.join().expect("mock");
        assert_eq!(
            recorded.len(),
            2,
            "no second lookup between meta and upload"
        );
        assert_eq!(recorded[1].method, "PATCH");
    }

    #[test]
    fn download_fetches_the_archive_media() {
        let (base_url, handle) = mock_server(vec![
            file_list("{\"id\": \"f1\"}"),
            (200, "ZIPBYTES".to_owned()),
        ]);
        let drive = drive_with(&base_url);

        let bytes = drive.download(test_now()).expect("download");

        assert_eq!(bytes, b"ZIPBYTES");
        let recorded = handle.join().expect("mock");
        assert!(recorded[1].path_with_query.contains("alt=media"));
    }

    #[test]
    fn download_without_a_backup_fails_with_a_clear_error() {
        let (base_url, handle) = mock_server(vec![file_list("")]);
        let drive = drive_with(&base_url);

        let err = drive.download(test_now()).expect_err("must fail");
        assert!(err.to_string().contains("no backup file"), "got: {err}");
        drop(handle.join().expect("mock"));
    }
}
