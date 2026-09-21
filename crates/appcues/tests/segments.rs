mod common;
use appcues::commands::segments;
use httpmock::Method::{DELETE, GET, PATCH, POST};
use httpmock::prelude::*;
use serde_json::json;

#[test]
fn create_posts_name_and_description() {
    let server = MockServer::start();
    let m = server.mock(|when, then| {
        when.method(POST)
            .path("/v2/accounts/acct1/segments")
            .header("authorization", common::AUTH)
            .json_body(json!({"name": "power users", "description": "actives"}));
        then.status(200)
            .json_body(json!({"id": "s1", "name": "power users"}));
    });
    let out = segments::create(&common::ctx(&server), "power users", Some("actives")).unwrap();
    m.assert();
    assert!(out.contains("s1"));
}

#[test]
fn create_omits_absent_description() {
    let server = MockServer::start();
    let m = server.mock(|when, then| {
        when.method(POST)
            .path("/v2/accounts/acct1/segments")
            .json_body(json!({"name": "minimal"}));
        then.status(200)
            .json_body(json!({"id": "s2", "name": "minimal"}));
    });
    segments::create(&common::ctx(&server), "minimal", None).unwrap();
    m.assert();
}

#[test]
fn update_patches_only_given_fields() {
    let server = MockServer::start();
    let m = server.mock(|when, then| {
        when.method(PATCH)
            .path("/v2/accounts/acct1/segments/s1")
            .json_body(json!({"name": "renamed"}));
        then.status(200)
            .json_body(json!({"id": "s1", "name": "renamed"}));
    });
    segments::update(&common::ctx(&server), "s1", Some("renamed"), None).unwrap();
    m.assert();
}

// NOTE: no integration test for delete-without---yes — it would depend on the
// ambient TTY state of the test process. The refusal path is deterministically
// unit-tested in commands/mod.rs (confirm_with); these tests cover the API call.

#[test]
fn delete_with_yes_calls_api() {
    let server = MockServer::start();
    let m = server.mock(|when, then| {
        when.method(DELETE)
            .path("/v2/accounts/acct1/segments/s1")
            .header("authorization", common::AUTH);
        then.status(204);
    });
    let out = segments::delete(&common::ctx(&server), "s1").unwrap();
    m.assert();
    assert!(out.contains("Deleted segment s1"));
}

#[test]
fn add_and_remove_users_post_id_lists() {
    let server = MockServer::start();
    let add = server.mock(|when, then| {
        when.method(POST)
            .path("/v2/accounts/acct1/segments/s1/add_user_ids")
            .json_body(json!({"user_ids": ["u1", "u2"]}));
        then.status(204);
    });
    let remove = server.mock(|when, then| {
        when.method(POST)
            .path("/v2/accounts/acct1/segments/s1/remove_user_ids")
            .json_body(json!({"user_ids": ["u3"]}));
        then.status(204);
    });
    let ctx = common::ctx(&server);
    segments::add_users(&ctx, "s1", &["u1".into(), "u2".into()]).unwrap();
    segments::remove_users(&ctx, "s1", &["u3".into()]).unwrap();
    add.assert();
    remove.assert();
}

#[test]
fn list_renders_columns() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(GET).path("/v2/accounts/acct1/segments");
        then.status(200).json_body(json!([{"id": "s1", "name": "power users", "description": "actives", "updated_at": "2026-01-01T00:00:00Z"}]));
    });
    let out = segments::list(&common::ctx(&server)).unwrap();
    assert!(out.contains("power users"));
}
