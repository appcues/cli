mod common;
use httpmock::prelude::*;
use serde_json::json;

const SPEC: &str = r#"{
  "metrics": ["nps_computed_score", "nps_respondents"],
  "dimensions": ["day"],
  "conditions": [["flow_id", "==", "-L6T65vAb2gUaRmfu9jv"]],
  "start_time": "2026-07-01",
  "end_time": "2026-08-01",
  "limit": 1000
}"#;

fn spec_file(dir: &tempfile::TempDir) -> String {
    let path = dir.path().join("q.json");
    std::fs::write(&path, SPEC).unwrap();
    path.display().to_string()
}

fn spec_value() -> serde_json::Value {
    serde_json::from_str(SPEC).unwrap()
}

#[test]
fn sync_query_posts_the_spec_verbatim() {
    let server = MockServer::start();
    let m = server.mock(|when, then| {
        when.method(POST)
            .path("/v2/accounts/acct1/analytics/query")
            .header("authorization", common::AUTH)
            .json_body(spec_value());
        // Sync 200s are a bare rows array (decided 2026-08-24).
        then.status(200).json_body(json!([
            {"day": "2026-07-01", "nps_computed_score": 42, "nps_respondents": 7}
        ]));
    });
    let dir = tempfile::tempdir().unwrap();
    let ctx = common::ctx(&server);
    let spec = appcues::commands::analytics::load_spec(&spec_file(&dir)).unwrap();
    let out = appcues::commands::analytics::query(&ctx, &spec, false).unwrap();
    m.assert();
    assert!(out.contains("42"));
}

#[test]
fn async_query_hits_the_exports_route_and_returns_job_id() {
    let server = MockServer::start();
    let m = server.mock(|when, then| {
        when.method(POST)
            .path("/v2/accounts/acct1/analytics/exports")
            .header("authorization", common::AUTH)
            .json_body(spec_value());
        then.status(202)
            .json_body(json!({"job_id": "j-uuid-1", "status": "queued"}));
    });
    let dir = tempfile::tempdir().unwrap();
    let ctx = common::ctx(&server);
    let spec = appcues::commands::analytics::load_spec(&spec_file(&dir)).unwrap();
    let out = appcues::commands::analytics::query(&ctx, &spec, true).unwrap();
    m.assert();
    assert!(out.contains("j-uuid-1") && out.contains("queued"));
}

#[test]
fn invalid_spec_json_is_rejected_before_any_request() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bad.json");
    std::fs::write(&path, "not json").unwrap();
    let err = appcues::commands::analytics::load_spec(&path.display().to_string()).unwrap_err();
    assert!(err.to_string().contains("not valid JSON"));
}

#[test]
fn missing_spec_file_is_named_in_the_error() {
    let err = appcues::commands::analytics::load_spec("/no/such/q.json").unwrap_err();
    assert!(format!("{err:#}").contains("/no/such/q.json"));
}

#[test]
fn dry_run_previews_the_post_without_sending() {
    let server = MockServer::start();
    let ctx = common::dry_run_ctx(&server);
    let spec = spec_value();
    let out = appcues::commands::analytics::query(&ctx, &spec, false).unwrap();
    assert!(out.contains("POST") && out.contains("analytics/query"));
    let out = appcues::commands::analytics::query(&ctx, &spec, true).unwrap();
    assert!(out.contains("POST") && out.contains("analytics/exports"));
}

#[test]
fn structured_400_surfaces_the_error_body() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(POST).path("/v2/accounts/acct1/analytics/query");
        then.status(400).json_body(json!({
            "error": true, "status": 400,
            "title": "invalid spec", "detail": "time range exceeds 90 days"
        }));
    });
    let ctx = common::ctx(&server);
    let err = appcues::commands::analytics::query(&ctx, &spec_value(), false).unwrap_err();
    let api = err
        .downcast_ref::<appcues::client::ApiError>()
        .expect("should be ApiError");
    assert_eq!(api.status, 400);
    assert_eq!(
        api.body.as_ref().unwrap()["detail"],
        "time range exceeds 90 days"
    );
}

// ---- analytics +compare ----

fn now() -> std::time::SystemTime {
    humantime::parse_rfc3339("2026-08-26T00:00:00Z").unwrap()
}

fn json_ctx(server: &MockServer) -> appcues::commands::Ctx {
    let mut c = common::ctx(server);
    c.format = appcues::output::Format::Json;
    c
}

fn mock_window<'a>(
    server: &'a MockServer,
    start: &str,
    end: &str,
    rows: serde_json::Value,
) -> httpmock::Mock<'a> {
    server.mock(move |when, then| {
        when.method(POST)
            .path("/v2/accounts/acct1/analytics/query")
            .header("authorization", common::AUTH)
            .json_body_partial(format!(
                r#"{{"start_time": "{start}", "end_time": "{end}", "metrics": ["events", "users"], "dimensions": ["name"]}}"#
            ));
        then.status(200).json_body(rows);
    })
}

fn compare_spec() -> serde_json::Value {
    json!({
        "metrics": ["events", "users"],
        "dimensions": ["name"],
        "conditions": [["name", "in", ["appcues:step_error", "appcues:flow_started"]]],
        "start_time": "2020-01-01",
        "end_time": "2020-01-02"
    })
}

#[test]
fn compare_runs_spec_for_both_windows_and_joins_rows_on_dimensions() {
    let server = MockServer::start();
    let cur = mock_window(
        &server,
        "2026-08-19T00:00:00Z",
        "2026-08-26T00:00:00Z",
        json!([
            {"name": "appcues:step_error", "events": 30, "users": 12},
            {"name": "appcues:flow_started", "events": 200, "users": 150}
        ]),
    );
    let prev = mock_window(
        &server,
        "2026-08-12T00:00:00Z",
        "2026-08-19T00:00:00Z",
        json!([
            {"name": "appcues:step_error", "events": 20, "users": 10},
            {"name": "appcues:nps_score", "events": 5, "users": 5}
        ]),
    );
    let out = appcues::commands::analytics::compare(&json_ctx(&server), &compare_spec(), 7, now())
        .unwrap();
    cur.assert();
    prev.assert();
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["period"]["start_time"], "2026-08-19T00:00:00Z");
    assert_eq!(v["period"]["end_time"], "2026-08-26T00:00:00Z");
    assert_eq!(v["previous_period"]["start_time"], "2026-08-12T00:00:00Z");
    assert_eq!(v["previous_period"]["end_time"], "2026-08-19T00:00:00Z");

    let rows = v["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 3, "union of both windows' dimension values");
    let by_name = |n: &str| {
        rows.iter()
            .find(|r| r["name"] == n)
            .unwrap_or_else(|| panic!("row {n}"))
    };
    let err = by_name("appcues:step_error");
    assert_eq!(err["current"]["events"], 30);
    assert_eq!(err["previous"]["events"], 20);
    assert_eq!(err["deltas"]["events_pct"], 50.0);
    assert_eq!(err["deltas"]["users_pct"], 20.0);
    // present now, absent before: previous is 0, delta is null (no baseline)
    let started = by_name("appcues:flow_started");
    assert_eq!(started["previous"]["events"], 0);
    assert_eq!(started["deltas"]["events_pct"], serde_json::Value::Null);
    // absent now, present before: current is 0, delta is -100
    let nps = by_name("appcues:nps_score");
    assert_eq!(nps["current"]["events"], 0);
    assert_eq!(nps["deltas"]["events_pct"], -100.0);
    // sorted by the first metric's current value, descending
    assert_eq!(rows[0]["name"], "appcues:flow_started");
    assert_eq!(rows[2]["name"], "appcues:nps_score");
}

#[test]
fn compare_rejects_a_raw_columns_spec() {
    let server = MockServer::start();
    let spec = json!({"columns": ["timestamp", "user_id"], "start_time": "2020-01-01", "end_time": "2020-01-02"});
    let err =
        appcues::commands::analytics::compare(&json_ctx(&server), &spec, 7, now()).unwrap_err();
    assert!(err.to_string().contains("metrics"), "{err}");
}

#[test]
fn compare_dry_run_prints_both_posts_and_sends_nothing() {
    let server = MockServer::start();
    let mut ctx = json_ctx(&server);
    ctx.dry_run = true;
    let out = appcues::commands::analytics::compare(&ctx, &compare_spec(), 7, now()).unwrap();
    assert_eq!(out.matches("POST").count(), 2, "{out}");
    assert!(out.contains("2026-08-19T00:00:00Z") && out.contains("2026-08-12T00:00:00Z"));
}

#[test]
fn compare_table_output_flattens_metrics_with_previous_and_delta_columns() {
    let server = MockServer::start();
    mock_window(
        &server,
        "2026-08-19T00:00:00Z",
        "2026-08-26T00:00:00Z",
        json!([{"name": "appcues:step_error", "events": 30, "users": 12}]),
    );
    mock_window(
        &server,
        "2026-08-12T00:00:00Z",
        "2026-08-19T00:00:00Z",
        json!([{"name": "appcues:step_error", "events": 20, "users": 10}]),
    );
    let out =
        appcues::commands::analytics::compare(&common::ctx(&server), &compare_spec(), 7, now())
            .unwrap();
    assert!(
        out.contains("2026-08-19T00:00:00Z → 2026-08-26T00:00:00Z"),
        "{out}"
    );
    for col in [
        "name",
        "events",
        "events_prev",
        "events_pct",
        "users",
        "users_prev",
        "users_pct",
    ] {
        assert!(out.contains(col), "missing column {col} in\n{out}");
    }
    assert!(out.contains("50"), "{out}");
}

#[test]
fn compare_dry_run_in_json_mode_is_one_json_document() {
    let server = MockServer::start();
    let mut ctx = json_ctx(&server);
    ctx.dry_run = true;
    let out = appcues::commands::analytics::compare(&ctx, &compare_spec(), 7, now()).unwrap();
    let v: serde_json::Value = serde_json::from_str(&out).expect("single JSON document");
    let reqs = v.as_array().expect("array of requests");
    assert_eq!(reqs.len(), 2);
    assert_eq!(reqs[0]["dry_run"], true);
    assert_eq!(reqs[0]["body"]["start_time"], "2026-08-19T00:00:00Z");
    assert_eq!(reqs[1]["body"]["start_time"], "2026-08-12T00:00:00Z");
}
