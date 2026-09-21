mod common;
use appcues::commands::tags;
use httpmock::prelude::*;
use serde_json::json;

#[test]
fn list_hits_endpoint_with_auth_and_renders_table() {
    let server = MockServer::start();
    let m = server.mock(|when, then| {
        when.method(GET)
            .path("/v2/accounts/acct1/tags")
            .header("authorization", common::AUTH);
        then.status(200)
            .json_body(json!([{"id": "t1", "name": "onboarding"}]));
    });
    let out = tags::list(&common::ctx(&server)).unwrap();
    m.assert();
    assert!(out.contains("t1") && out.contains("onboarding"));
}

#[test]
fn get_renders_single_tag() {
    let server = MockServer::start();
    let m = server.mock(|when, then| {
        when.method(GET)
            .path("/v2/accounts/acct1/tags/t1")
            .header("authorization", common::AUTH);
        then.status(200)
            .json_body(json!({"id": "t1", "name": "onboarding"}));
    });
    let out = tags::get(&common::ctx(&server), "t1").unwrap();
    m.assert();
    assert!(out.contains("name: onboarding"));
}

#[test]
fn api_error_propagates() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(GET).path("/v2/accounts/acct1/tags/nope");
        then.status(404).json_body(
            json!({"error": true, "status": 404, "title": "Not Found", "detail": "tag not found"}),
        );
    });
    let err = tags::get(&common::ctx(&server), "nope").unwrap_err();
    assert!(err.to_string().contains("tag not found"));
}
