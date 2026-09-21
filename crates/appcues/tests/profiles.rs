mod common;
use appcues::commands::profiles;
use httpmock::prelude::*;
use serde_json::json;

#[test]
fn status_verifies_credentials_with_a_cheap_get() {
    let server = MockServer::start();
    let m = server.mock(|when, then| {
        when.method(GET)
            .path("/v2/accounts/acct1/tags")
            .header("authorization", common::AUTH);
        then.status(200).json_body(json!([]));
    });
    let out = profiles::status(&common::ctx(&server)).unwrap();
    m.assert();
    assert!(out.contains("acct1") && out.contains("OK"));
    // Status names the API origin it verified against, so a stray
    // APPCUES_BASE_URL or EU profile is visible at a glance.
    assert!(out.contains(&server.base_url()));
}

#[test]
fn status_reports_bad_credentials() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(GET).path("/v2/accounts/acct1/tags");
        then.status(401).json_body(
            json!({"error": true, "status": 401, "title": "Unauthorized", "detail": "bad credentials"}),
        );
    });
    let err = profiles::status(&common::ctx(&server)).unwrap_err();
    assert!(err.to_string().contains("401"));
}
