mod common;
use httpmock::prelude::*;

#[test]
fn dry_run_prints_request_and_sends_nothing() {
    let server = MockServer::start();
    let m = server.mock(|when, then| {
        when.method(POST).path_matches(Regex::new(".*").unwrap());
        then.status(200);
    });
    let ctx = common::dry_run_ctx(&server);
    let out = appcues::commands::flows::publish(&ctx, "f1").unwrap();
    assert!(out.contains("POST") && out.contains("/v2/accounts/acct1/flows/f1/publish"));
    m.assert_hits(0);
}

#[test]
fn dry_run_delete_skips_confirmation_prompt() {
    let server = MockServer::start();
    let ctx = common::dry_run_ctx(&server);
    // interactive prompting would normally apply; dry-run returns first
    let mut ctx = ctx;
    ctx.interactive = true;
    let out = appcues::commands::segments::delete(&ctx, "s1").unwrap();
    assert!(out.contains("DELETE") && out.contains("segments/s1"));
}

#[test]
fn dry_run_json_output_is_structured() {
    let server = MockServer::start();
    let mut ctx = common::dry_run_ctx(&server);
    ctx.format = appcues::output::Format::Json;
    let out = appcues::commands::segments::create(&ctx, "power users", None).unwrap();
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["dry_run"], true);
    assert_eq!(v["method"], "POST");
    assert_eq!(v["body"]["name"], "power users");
}
