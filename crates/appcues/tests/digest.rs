mod common;

use appcues::commands::flows;
use appcues::output::Format;
use httpmock::prelude::*;
use serde_json::{Value, json};
use std::time::SystemTime;

fn json_ctx(server: &MockServer) -> appcues::commands::Ctx {
    let mut c = common::ctx(server);
    c.format = Format::Json;
    c
}

fn now() -> SystemTime {
    humantime::parse_rfc3339("2026-08-26T00:00:00Z").unwrap()
}

/// One (flow_id, event name) aggregate row, the shape analytics/query
/// returns for metrics ["events","users"] × dimensions ["flow_id","name"].
fn row(flow_id: &str, name: &str, events: u64, users: u64) -> Value {
    json!({"flow_id": flow_id, "name": name, "events": events, "users": users})
}

fn mock_flows(server: &MockServer) -> httpmock::Mock<'_> {
    server.mock(|when, then| {
        when.method(GET)
            .path("/v2/accounts/acct1/flows")
            .header("authorization", common::AUTH);
        then.status(200).json_body(json!([
            {"id": "f1", "name": "Welcome", "published": true},
            {"id": "f2", "name": "Quiet flow", "published": true},
            {"id": "f3", "name": "Draft", "published": false},
        ]));
    })
}

fn mock_query<'a>(server: &'a MockServer, start_time: &str, rows: Value) -> httpmock::Mock<'a> {
    server.mock(move |when, then| {
        when.method(POST)
            .path("/v2/accounts/acct1/analytics/query")
            .json_body_partial(format!(r#"{{"start_time": "{start_time}"}}"#));
        then.status(200).json_body(rows);
    })
}

#[test]
fn digest_joins_periods_and_computes_deltas() {
    let server = MockServer::start();
    let flows_mock = mock_flows(&server);
    let cur = mock_query(
        &server,
        "2026-08-19T00:00:00Z",
        json!([
            row("f1", "appcues:flow_started", 120, 80),
            row("f1", "appcues:flow_completed", 90, 70),
            row("f1", "appcues:flow_skipped", 10, 9),
            row("f1", "appcues:step_error", 2, 2),
        ]),
    );
    let prev = mock_query(
        &server,
        "2026-08-12T00:00:00Z",
        json!([
            row("f1", "appcues:flow_started", 100, 70),
            row("f1", "appcues:flow_completed", 60, 50),
        ]),
    );
    let out = flows::digest(&json_ctx(&server), 7, now()).unwrap();
    flows_mock.assert();
    cur.assert();
    prev.assert();
    let v: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["period"]["start_time"], "2026-08-19T00:00:00Z");
    assert_eq!(v["period"]["end_time"], "2026-08-26T00:00:00Z");
    assert_eq!(v["previous_period"]["start_time"], "2026-08-12T00:00:00Z");
    let f1 = &v["flows"][0];
    assert_eq!(f1["flow_id"], "f1");
    assert_eq!(f1["name"], "Welcome");
    assert_eq!(f1["shown"], 120);
    assert_eq!(f1["completed"], 90);
    assert_eq!(f1["skipped"], 10);
    assert_eq!(f1["errors"], 2);
    assert_eq!(f1["unique_users"], 80);
    assert_eq!(f1["completion_rate"], 75.0);
    assert_eq!(f1["previous"]["shown"], 100);
    assert_eq!(f1["previous"]["completion_rate"], 60.0);
    assert_eq!(f1["deltas"]["shown_pct"], 20.0);
    assert_eq!(f1["deltas"]["completion_rate_pts"], 15.0);
    // users 70 -> 80 = +14.3%
    assert_eq!(f1["deltas"]["unique_users_pct"], 14.3);
}

#[test]
fn digest_zero_fills_quiet_flows_and_excludes_drafts() {
    let server = MockServer::start();
    mock_flows(&server);
    mock_query(&server, "2026-08-19T00:00:00Z", json!([]));
    mock_query(&server, "2026-08-12T00:00:00Z", json!([]));
    let out = flows::digest(&json_ctx(&server), 7, now()).unwrap();
    let v: Value = serde_json::from_str(&out).unwrap();
    let listed: Vec<&str> = v["flows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["flow_id"].as_str().unwrap())
        .collect();
    assert!(listed.contains(&"f1") && listed.contains(&"f2"));
    assert!(!listed.contains(&"f3"), "draft flows stay out");
    let quiet = &v["flows"][0];
    assert_eq!(quiet["shown"], 0);
    assert_eq!(quiet["completion_rate"], Value::Null);
    assert_eq!(quiet["deltas"]["shown_pct"], Value::Null);
}

#[test]
fn digest_sorts_by_completion_rate_desc_nulls_last() {
    let server = MockServer::start();
    mock_flows(&server);
    mock_query(
        &server,
        "2026-08-19T00:00:00Z",
        json!([
            row("f1", "appcues:flow_started", 100, 50),
            row("f1", "appcues:flow_completed", 20, 10),
            row("f2", "appcues:flow_started", 10, 5),
            row("f2", "appcues:flow_completed", 9, 5),
        ]),
    );
    mock_query(&server, "2026-08-12T00:00:00Z", json!([]));
    let out = flows::digest(&json_ctx(&server), 7, now()).unwrap();
    let v: Value = serde_json::from_str(&out).unwrap();
    // f2 at 90% ranks above f1 at 20%; a null-rate flow sorts last.
    assert_eq!(v["flows"][0]["flow_id"], "f2");
    assert_eq!(v["flows"][1]["flow_id"], "f1");
}

#[test]
fn digest_table_output_renders_flow_rows() {
    let server = MockServer::start();
    mock_flows(&server);
    mock_query(
        &server,
        "2026-08-19T00:00:00Z",
        json!([
            row("f1", "appcues:flow_started", 120, 80),
            row("f1", "appcues:flow_completed", 90, 70),
        ]),
    );
    mock_query(&server, "2026-08-12T00:00:00Z", json!([]));
    let out = flows::digest(&common::ctx(&server), 7, now()).unwrap();
    assert!(out.contains("Welcome") && out.contains("75"), "got: {out}");
    assert!(out.contains("2026-08-19"), "period shown, got: {out}");
}

#[test]
fn digest_dry_run_prints_both_specs_and_sends_nothing() {
    let server = MockServer::start();
    let flows_mock = mock_flows(&server); // must NOT be hit
    let mut ctx = json_ctx(&server);
    ctx.dry_run = true;
    let out = flows::digest(&ctx, 7, now()).unwrap();
    assert!(out.contains("2026-08-19T00:00:00Z") && out.contains("2026-08-12T00:00:00Z"));
    assert!(out.contains("appcues:flow_started"));
    let v: Value = serde_json::from_str(&out).expect("json mode: one JSON document");
    assert_eq!(v.as_array().map(Vec::len), Some(2));
    flows_mock.assert_hits(0);
}

#[test]
fn digest_api_error_mid_composite_keeps_exit_contract() {
    let server = MockServer::start();
    mock_flows(&server);
    // the current-period query 404s; the previous-period query never fires
    let bad = server.mock(|when, then| {
        when.method(POST).path("/v2/accounts/acct1/analytics/query");
        then.status(404).json_body(
            json!({"error": true, "status": 404, "title": "Not Found", "detail": "no analytics"}),
        );
    });
    let err = flows::digest(&json_ctx(&server), 7, now()).unwrap_err();
    bad.assert_hits(1);
    let api = err
        .downcast_ref::<appcues::client::ApiError>()
        .expect("composite failures stay downcastable ApiErrors");
    assert_eq!(api.status, 404);
}
