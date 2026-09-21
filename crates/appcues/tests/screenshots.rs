mod common;
use appcues::commands::screenshots;
use httpmock::prelude::*;
use serde_json::json;

#[test]
fn downloads_zip_with_auth_to_the_given_path() {
    let server = MockServer::start();
    let m = server.mock(|when, then| {
        when.method(GET)
            .path("/v2/accounts/acct1/screenshots/f1")
            .header("authorization", common::AUTH);
        then.status(200).body("PK\x03\x04fake-zip-bytes");
    });
    let dir = tempfile::tempdir().unwrap();
    let out_path = dir.path().join("shots.zip");
    let out = screenshots::download(&common::ctx(&server), "f1", Some(&out_path)).unwrap();
    m.assert();
    assert_eq!(
        std::fs::read(&out_path).unwrap(),
        b"PK\x03\x04fake-zip-bytes"
    );
    assert!(out.contains(&out_path.display().to_string()), "got: {out}");
}

#[test]
fn api_error_propagates_and_leaves_no_file() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(GET).path("/v2/accounts/acct1/screenshots/nope");
        then.status(404).json_body(
            json!({"error": true, "status": 404, "title": "Not Found", "detail": "content not found"}),
        );
    });
    let dir = tempfile::tempdir().unwrap();
    let out_path = dir.path().join("shots.zip");
    let err = screenshots::download(&common::ctx(&server), "nope", Some(&out_path)).unwrap_err();
    assert!(err.to_string().contains("content not found"), "got: {err}");
    assert!(!out_path.exists());
    assert!(!dir.path().join("shots.zip.part").exists());
}

#[test]
fn dry_run_previews_without_any_request() {
    // No mocks registered: any request to the mock server would 404 and fail.
    let server = MockServer::start();
    let out = screenshots::download(&common::dry_run_ctx(&server), "f1", None).unwrap();
    assert!(
        out.contains("DRY RUN: GET /v2/accounts/acct1/screenshots/f1"),
        "got: {out}"
    );
}
