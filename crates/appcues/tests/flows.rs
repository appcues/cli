mod common;
use appcues::commands::flows;
use httpmock::prelude::*;
use serde_json::json;

#[test]
fn list_renders_expected_columns() {
    let server = MockServer::start();
    let m = server.mock(|when, then| {
        when.method(GET)
            .path("/v2/accounts/acct1/flows")
            .header("authorization", common::AUTH);
        then.status(200).json_body(json!([
            {"id": "f1", "name": "Welcome", "published": true, "updated_at": "2026-01-01T00:00:00Z"}
        ]));
    });
    let out = flows::list(&common::ctx(&server)).unwrap();
    m.assert();
    assert!(out.contains("Welcome") && out.contains("true"));
}

#[test]
fn get_returns_flow_details() {
    let server = MockServer::start();
    let m = server.mock(|when, then| {
        when.method(GET)
            .path("/v2/accounts/acct1/flows/f1")
            .header("authorization", common::AUTH);
        then.status(200)
            .json_body(json!({"id": "f1", "name": "Welcome", "published": false}));
    });
    let out = flows::get(&common::ctx(&server), "f1").unwrap();
    m.assert();
    assert!(out.contains("name     : Welcome"), "got: {out}");
}

#[test]
fn publish_posts_and_reports_success() {
    let server = MockServer::start();
    let m = server.mock(|when, then| {
        when.method(POST)
            .path("/v2/accounts/acct1/flows/f1/publish")
            .header("authorization", common::AUTH);
        then.status(204);
    });
    let out = flows::publish(&common::ctx(&server), "f1").unwrap();
    m.assert();
    assert!(out.contains("Published flow f1"));
}

#[test]
fn unpublish_posts_to_unpublish_path() {
    let server = MockServer::start();
    let m = server.mock(|when, then| {
        when.method(POST)
            .path("/v2/accounts/acct1/flows/f1/unpublish")
            .header("authorization", common::AUTH);
        then.status(204);
    });
    let out = flows::unpublish(&common::ctx(&server), "f1").unwrap();
    m.assert();
    assert!(out.contains("Unpublished flow f1"));
}
