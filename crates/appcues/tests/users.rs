mod common;
use appcues::commands::{parse_attrs, users};
use httpmock::Method::PATCH;
use httpmock::prelude::*;
use serde_json::json;

#[test]
fn get_reads_profile() {
    let server = MockServer::start();
    let m = server.mock(|when, then| {
        when.method(GET)
            .path("/v2/accounts/acct1/users/u1/profile")
            .header("authorization", common::AUTH);
        then.status(200)
            .json_body(json!({"user_id": "u1", "plan": "pro"}));
    });
    let out = users::get(&common::ctx(&server), "u1").unwrap();
    m.assert();
    assert!(out.contains("plan   : pro"));
}

#[test]
fn update_patches_typed_attributes() {
    let server = MockServer::start();
    let m = server.mock(|when, then| {
        when.method(PATCH)
            .path("/v2/accounts/acct1/users/u1/profile")
            .json_body(json!({"plan": "pro", "seats": 5}));
        then.status(200).json_body(json!({"user_id": "u1"}));
    });
    users::update(
        &common::ctx(&server),
        "u1",
        parse_attrs(&["plan=pro".into(), "seats=5".into()]).unwrap(),
    )
    .unwrap();
    m.assert();
}

// NOTE: no integration test for delete-without---yes — it would depend on the
// ambient TTY state of the test process; the refusal path is unit-tested in
// commands/mod.rs (confirm_with).

#[test]
fn delete_with_yes_calls_api() {
    let server = MockServer::start();
    let m = server.mock(|when, then| {
        when.method(DELETE)
            .path("/v2/accounts/acct1/users/u1/profile");
        then.status(204);
    });
    let out = users::delete(&common::ctx(&server), "u1").unwrap();
    m.assert();
    assert!(out.contains("Deleted profile for user u1"));
}

#[test]
fn events_passes_limit_query() {
    let server = MockServer::start();
    let m = server.mock(|when, then| {
        when.method(GET)
            .path("/v2/accounts/acct1/users/u1/events")
            .query_param("limit", "5");
        then.status(200)
            .json_body(json!([{"name": "flow_started", "timestamp": "2026-01-01T00:00:00Z"}]));
    });
    let out = users::events(&common::ctx(&server), "u1", Some(5)).unwrap();
    m.assert();
    assert!(out.contains("flow_started"));
}

#[test]
fn track_sends_name_timestamp_and_attributes() {
    let server = MockServer::start();
    let m = server.mock(|when, then| {
        when.method(POST)
            .path("/v2/accounts/acct1/users/u1/events")
            .json_body(json!({
                "name": "upgraded",
                "timestamp": "2026-08-12T00:00:00Z",
                "attributes": {"plan": "pro"}
            }));
        then.status(204);
    });
    let out = users::track(
        &common::ctx(&server),
        "u1",
        "upgraded",
        Some("2026-08-12T00:00:00Z"),
        parse_attrs(&["plan=pro".into()]).unwrap(),
    )
    .unwrap();
    m.assert();
    assert!(out.contains("Tracked event 'upgraded'"));
}

#[test]
fn track_defaults_timestamp_to_now() {
    let server = MockServer::start();
    let m = server.mock(|when, then| {
        when.method(POST).path("/v2/accounts/acct1/users/u1/events");
        then.status(204);
    });
    users::track(
        &common::ctx(&server),
        "u1",
        "ping",
        None,
        Default::default(),
    )
    .unwrap();
    m.assert(); // timestamp value is "now", so we only assert the call happened
}
