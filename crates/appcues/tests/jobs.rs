mod common;
use httpmock::prelude::*;
use serde_json::json;
use std::time::Duration;

#[test]
fn get_hits_the_analytics_exports_route() {
    let server = MockServer::start();
    let m = server.mock(|when, then| {
        when.method(GET)
            .path("/v2/accounts/acct1/analytics/exports/j1")
            .header("authorization", common::AUTH);
        then.status(200)
            .json_body(json!({"job_id": "j1", "status": "running"}));
    });
    let ctx = common::ctx(&server);
    let out = appcues::commands::jobs::get(&ctx, "j1").unwrap();
    m.assert();
    assert!(out.contains("running"));
}

#[test]
fn get_surfaces_a_plain_404_for_unknown_or_legacy_job_ids() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(GET)
            .path("/v2/accounts/acct1/analytics/exports/legacy-1");
        then.status(404).json_body(json!({
            "error": true, "status": 404,
            "title": "Not Found", "detail": "no such export job"
        }));
    });
    let ctx = common::ctx(&server);
    let err = appcues::commands::jobs::get(&ctx, "legacy-1").unwrap_err();
    let api = err
        .downcast_ref::<appcues::client::ApiError>()
        .expect("should be ApiError");
    assert_eq!(api.status, 404);
}

#[test]
fn wait_returns_the_final_body_when_done() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(GET)
            .path("/v2/accounts/acct1/analytics/exports/j1");
        then.status(200).json_body(json!({
            "job_id": "j1",
            "status": "done",
            "download_url": "https://s3.example.com/presigned/j1.json",
            "expires_at": "2026-08-31T00:00:00Z"
        }));
    });
    let ctx = common::ctx(&server);
    let out = appcues::commands::jobs::wait(&ctx, "j1", Duration::from_secs(5)).unwrap();
    assert!(out.contains("done") && out.contains("presigned/j1.json"));
}

#[test]
fn wait_surfaces_the_failure_reason() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(GET)
            .path("/v2/accounts/acct1/analytics/exports/j1");
        then.status(200).json_body(json!({
            "job_id": "j1", "status": "failed", "failure_reason": "query timeout"
        }));
    });
    let ctx = common::ctx(&server);
    let err = appcues::commands::jobs::wait(&ctx, "j1", Duration::from_secs(5)).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("failed") && msg.contains("query timeout"));
}

#[test]
fn download_streams_the_result_to_a_file_without_auth() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(GET)
            .path("/v2/accounts/acct1/analytics/exports/j1")
            .header("authorization", common::AUTH);
        then.status(200).json_body(json!({
            "job_id": "j1", "status": "done",
            "download_url": server.url("/presigned/j1.json"),
        }));
    });
    // The presigned fetch must NOT carry the API's Basic credentials.
    let dl = server.mock(|when, then| {
        when.method(GET).path("/presigned/j1.json").matches(|req| {
            !req.headers
                .iter()
                .flatten()
                .any(|(k, _)| k.eq_ignore_ascii_case("authorization"))
        });
        then.status(200).body("[{\"name\":\"e1\"}]");
    });
    let dir = tempfile::tempdir().unwrap();
    let out_path = dir.path().join("result.json");
    let ctx = common::ctx(&server);
    let msg =
        appcues::commands::jobs::download(&ctx, "j1", Some(&out_path), Duration::from_secs(5))
            .unwrap();
    dl.assert();
    assert_eq!(
        std::fs::read_to_string(&out_path).unwrap(),
        "[{\"name\":\"e1\"}]"
    );
    assert!(msg.contains(&out_path.display().to_string()));
}

#[test]
fn download_defaults_the_filename_to_the_job_id() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(GET)
            .path("/v2/accounts/acct1/analytics/exports/j-default");
        then.status(200).json_body(json!({
            "job_id": "j-default", "status": "done",
            "download_url": server.url("/presigned/j-default.json"),
        }));
    });
    server.mock(|when, then| {
        when.method(GET).path("/presigned/j-default.json");
        then.status(200).body("[]");
    });
    let dir = tempfile::tempdir().unwrap();
    let prev = std::env::current_dir().unwrap();
    std::env::set_current_dir(dir.path()).unwrap();
    let ctx = common::ctx(&server);
    let msg = appcues::commands::jobs::download(&ctx, "j-default", None, Duration::from_secs(5));
    std::env::set_current_dir(prev).unwrap();
    assert!(msg.unwrap().contains("j-default.json"));
    assert_eq!(
        std::fs::read_to_string(dir.path().join("j-default.json")).unwrap(),
        "[]"
    );
}

#[test]
fn failed_download_preserves_an_existing_output_file() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(GET)
            .path("/v2/accounts/acct1/analytics/exports/j1");
        then.status(200).json_body(json!({
            "job_id": "j1", "status": "done",
            "download_url": server.url("/presigned/j1.json"),
        }));
    });
    server.mock(|when, then| {
        when.method(GET).path("/presigned/j1.json");
        then.status(403).body("expired");
    });
    let dir = tempfile::tempdir().unwrap();
    let out_path = dir.path().join("result.json");
    std::fs::write(&out_path, "precious previous export").unwrap();
    let ctx = common::ctx(&server);
    let err =
        appcues::commands::jobs::download(&ctx, "j1", Some(&out_path), Duration::from_secs(5))
            .unwrap_err();
    assert!(err.to_string().contains("download"));
    assert_eq!(
        std::fs::read_to_string(&out_path).unwrap(),
        "precious previous export"
    );
    assert!(!dir.path().join("result.json.part").exists());
}

#[test]
fn download_surfaces_the_failure_reason() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(GET)
            .path("/v2/accounts/acct1/analytics/exports/j1");
        then.status(200).json_body(json!({
            "job_id": "j1", "status": "failed", "failure_reason": "export expired"
        }));
    });
    let ctx = common::ctx(&server);
    let err = appcues::commands::jobs::download(&ctx, "j1", None, Duration::from_secs(5))
        .unwrap_err()
        .to_string();
    assert!(err.contains("failed") && err.contains("export expired"));
}

#[test]
fn download_dry_run_previews_without_touching_network_or_disk() {
    let server = MockServer::start();
    let ctx = common::dry_run_ctx(&server);
    let out = appcues::commands::jobs::download(&ctx, "j1", None, Duration::ZERO).unwrap();
    assert!(out.contains("analytics/exports/j1"));
    assert!(!std::path::Path::new("j1.json").exists());
}

#[test]
fn wait_times_out_while_queued() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(GET)
            .path("/v2/accounts/acct1/analytics/exports/j1");
        then.status(200)
            .json_body(json!({"job_id": "j1", "status": "queued"}));
    });
    let ctx = common::ctx(&server);
    let err = appcues::commands::jobs::wait(&ctx, "j1", Duration::ZERO).unwrap_err();
    assert!(err.to_string().contains("timed out"));
}
