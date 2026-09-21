mod common;
use appcues::commands::checklists;
use httpmock::prelude::*;
use serde_json::json;

#[test]
fn list_hits_endpoint_with_auth_and_renders_table() {
    let server = MockServer::start();
    let m = server.mock(|when, then| {
        when.method(GET)
            .path("/v2/accounts/acct1/checklists")
            .header("authorization", common::AUTH);
        then.status(200).json_body(json!([
            {"id": "c1", "name": "Onboarding checklist", "published": true, "updated_at": "2026-01-01T00:00:00Z"}
        ]));
    });
    let out = checklists::list(&common::ctx(&server)).unwrap();
    m.assert();
    assert!(out.contains("Onboarding checklist") && out.contains("true"));
}

#[test]
fn get_renders_single_checklist() {
    let server = MockServer::start();
    let m = server.mock(|when, then| {
        when.method(GET)
            .path("/v2/accounts/acct1/checklists/c1")
            .header("authorization", common::AUTH);
        then.status(200)
            .json_body(json!({"id": "c1", "name": "Onboarding checklist", "published": false}));
    });
    let out = checklists::get(&common::ctx(&server), "c1").unwrap();
    m.assert();
    assert!(out.contains("Onboarding checklist"), "got: {out}");
}

#[test]
fn publish_posts_and_reports_success() {
    let server = MockServer::start();
    let m = server.mock(|when, then| {
        when.method(POST)
            .path("/v2/accounts/acct1/checklists/c1/publish")
            .header("authorization", common::AUTH);
        then.status(204);
    });
    let out = checklists::publish(&common::ctx(&server), "c1").unwrap();
    m.assert();
    assert!(out.contains("Published checklist c1"), "got: {out}");
}

#[test]
fn unpublish_posts_to_unpublish_path() {
    let server = MockServer::start();
    let m = server.mock(|when, then| {
        when.method(POST)
            .path("/v2/accounts/acct1/checklists/c1/unpublish")
            .header("authorization", common::AUTH);
        then.status(204);
    });
    let out = checklists::unpublish(&common::ctx(&server), "c1").unwrap();
    m.assert();
    assert!(out.contains("Unpublished checklist c1"), "got: {out}");
}

#[test]
fn api_error_propagates() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(GET).path("/v2/accounts/acct1/checklists/nope");
        then.status(404).json_body(
            json!({"error": true, "status": 404, "title": "Not Found", "detail": "checklist not found"}),
        );
    });
    let err = checklists::get(&common::ctx(&server), "nope").unwrap_err();
    assert!(err.to_string().contains("checklist not found"));
}
