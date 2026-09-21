mod common;

use appcues::commands::tools::{CallOpts, ToolError, call, describe, list};
use appcues::output::Format;
use httpmock::prelude::*;
use serde_json::{Value, json};

fn call_opts() -> CallOpts<'static> {
    CallOpts {
        input: None,
        input_file: None,
        attrs: &[],
        raw: false,
        out: None,
    }
}

fn listing_entry(read_only: bool) -> Value {
    json!({
        "name": "list_campaigns",
        "description": "List the campaigns in the account with their status",
        "inputSchema": {"type": "object", "properties": {}, "required": []},
        "annotations": {
            "title": "List Campaigns",
            "readOnlyHint": read_only,
            "destructiveHint": !read_only,
            "openWorldHint": false
        },
        "role": "account_readonly"
    })
}

/// A summary-view entry: name, role, description only (section 2.3).
fn summary_entry() -> Value {
    json!({
        "name": "list_campaigns",
        "description": "List the campaigns in the account with their status",
        "role": "account_readonly"
    })
}

#[test]
fn list_defaults_to_the_summary_view() {
    let server = MockServer::start();
    let m = server.mock(|when, then| {
        when.method(GET)
            .path("/v1/tools")
            .query_param("view", "summary")
            .header("appcues-api-key", common::KEY_AUTH);
        then.status(200)
            .json_body(json!({"tools": [summary_entry()]}));
    });
    let out = list(&common::ctx(&server), false).unwrap();
    m.assert();
    assert!(out.contains("list_campaigns"));
    assert!(out.contains("account_readonly"));
    assert!(!out.contains("title")); // no annotations in the summary view
}

#[test]
fn list_full_fetches_complete_entries() {
    let server = MockServer::start();
    let summary = server.mock(|when, then| {
        when.method(GET)
            .path("/v1/tools")
            .query_param("view", "summary");
        then.status(200).json_body(json!({"tools": []}));
    });
    let m = server.mock(|when, then| {
        when.method(GET)
            .path("/v1/tools")
            .header("appcues-api-key", common::KEY_AUTH);
        then.status(200)
            .json_body(json!({"tools": [listing_entry(true)]}));
    });
    let out = list(&common::ctx(&server), true).unwrap();
    summary.assert_hits(0); // --full sends no view parameter
    m.assert();
    assert!(out.contains("list_campaigns"));
    assert!(out.contains("List Campaigns")); // title column is back
}

#[test]
fn list_json_prints_the_tools_array() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(GET)
            .path("/v1/tools")
            .query_param("view", "summary");
        then.status(200)
            .json_body(json!({"tools": [summary_entry()]}));
    });
    let mut ctx = common::ctx(&server);
    ctx.format = Format::Json;
    let out = list(&ctx, false).unwrap();
    let v: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v[0]["name"], "list_campaigns");
    assert_eq!(v[0]["role"], "account_readonly");
    assert!(v[0].get("inputSchema").is_none()); // summary entries, verbatim
}

#[test]
fn list_full_json_carries_input_schema() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(GET).path("/v1/tools");
        then.status(200)
            .json_body(json!({"tools": [listing_entry(true)]}));
    });
    let mut ctx = common::ctx(&server);
    ctx.format = Format::Json;
    let out = list(&ctx, true).unwrap();
    let v: Value = serde_json::from_str(&out).unwrap();
    assert!(v[0]["inputSchema"].is_object());
    assert_eq!(v[0]["annotations"]["title"], "List Campaigns");
}

#[test]
fn describe_prints_one_entry() {
    let server = MockServer::start();
    let m = server.mock(|when, then| {
        when.method(GET)
            .path("/v1/tools/list_campaigns")
            .header("appcues-api-key", common::KEY_AUTH);
        then.status(200).json_body(listing_entry(true));
    });
    let mut ctx = common::ctx(&server);
    ctx.format = Format::Json;
    let out = describe(&ctx, "list_campaigns").unwrap();
    m.assert();
    let v: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["annotations"]["title"], "List Campaigns");
}

#[test]
fn describe_unknown_tool_is_an_api_404() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(GET).path("/v1/tools/nope");
        then.status(404)
            .json_body(json!({"error": "tool_not_found", "tool": "nope"}));
    });
    let err = describe(&common::ctx(&server), "nope").unwrap_err();
    let api = err
        .downcast_ref::<appcues::client::ApiError>()
        .expect("should be ApiError");
    assert_eq!(api.status, 404);
}

#[test]
fn call_renders_data_when_present() {
    let server = MockServer::start();
    let m = server.mock(|when, then| {
        when.method(POST)
            .path("/v1/tools/list_campaigns")
            .header("appcues-api-key", common::KEY_AUTH)
            .json_body(json!({"arguments": {}}));
        then.status(200).json_body(json!({
            "content": [{"type": "text", "text": "2 campaigns"}],
            "data": [
                {"type": "campaign", "id": "c1", "name": "Launch", "link": true},
                {"type": "campaign", "id": "c2", "name": "Onboard", "link": true}
            ],
            "isError": false
        }));
    });
    let out = call(&common::ctx(&server), "list_campaigns", &call_opts()).unwrap();
    m.assert();
    assert!(out.contains("c1") && out.contains("Launch"));
    assert!(out.contains("c2") && out.contains("Onboard"));
}

#[test]
fn call_parses_json_carried_in_text() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(POST).path("/v1/tools/get_campaign");
        then.status(200).json_body(json!({
            "content": [{"type": "text", "text": "{\"id\":\"c1\",\"status\":\"active\"}"}],
            "isError": false
        }));
    });
    let mut ctx = common::ctx(&server);
    ctx.format = Format::Json;
    let out = call(&ctx, "get_campaign", &call_opts()).unwrap();
    let v: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["status"], "active"); // parsed, not a JSON-encoded string
}

#[test]
fn call_prints_prose_text_verbatim() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(POST).path("/v1/tools/get_campaign");
        then.status(200).json_body(json!({
            "content": [{"type": "text", "text": "Campaign Launch is active."}],
            "isError": false
        }));
    });
    let out = call(&common::ctx(&server), "get_campaign", &call_opts()).unwrap();
    assert_eq!(out, "Campaign Launch is active.");
}

#[test]
fn call_raw_prints_the_whole_envelope() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(POST).path("/v1/tools/get_campaign");
        then.status(200).json_body(json!({
            "content": [{"type": "text", "text": "prose"}],
            "data": [{"type": "campaign", "id": "c1", "name": "Launch"}],
            "isError": false
        }));
    });
    let mut opts = call_opts();
    opts.raw = true;
    let out = call(&common::ctx(&server), "get_campaign", &opts).unwrap();
    let v: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["isError"], false);
    assert_eq!(v["content"][0]["text"], "prose");
    assert_eq!(v["data"][0]["id"], "c1");
}

#[test]
fn call_is_error_becomes_a_tool_error() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(POST).path("/v1/tools/create_campaign");
        then.status(200).json_body(json!({
            "content": [{"type": "text", "text": "objective_id not found"}],
            "isError": true
        }));
    });
    let err = call(&common::ctx(&server), "create_campaign", &call_opts()).unwrap_err();
    let tool = err
        .downcast_ref::<ToolError>()
        .expect("should be ToolError (exit 4)");
    assert_eq!(tool.tool, "create_campaign");
    assert!(tool.message.contains("objective_id not found"));
    assert_eq!(tool.envelope["isError"], true); // embedded for the stderr line
}

#[test]
fn call_writes_image_items_to_out_dir() {
    let server = MockServer::start();
    // base64("png-bytes")
    server.mock(|when, then| {
        when.method(POST)
            .path("/v1/tools/get_experience_screenshot");
        then.status(200).json_body(json!({
            "content": [
                {"type": "text", "text": "Screenshot of step 1"},
                {"type": "image", "data": "cG5nLWJ5dGVz", "mimeType": "image/png"}
            ],
            "isError": false
        }));
    });
    let dir = tempfile::tempdir().unwrap();
    let mut opts = call_opts();
    opts.out = Some(dir.path());
    let out = call(&common::ctx(&server), "get_experience_screenshot", &opts).unwrap();
    assert!(out.contains("Screenshot of step 1"));
    assert!(out.contains("Saved "));
    let files: Vec<_> = std::fs::read_dir(dir.path()).unwrap().collect();
    assert_eq!(files.len(), 1);
    let path = files[0].as_ref().unwrap().path();
    let name = path.file_name().unwrap().to_string_lossy().to_string();
    assert!(name.starts_with("get_experience_screenshot-") && name.ends_with(".png"));
    assert_eq!(std::fs::read(&path).unwrap(), b"png-bytes");
}

/// Runs the real binary with stdin piped, so the off-terminal branch is
/// deterministic even when `cargo test` itself runs on a terminal (the
/// in-process path would consult the test runner's own stdin and could
/// block on the prompt).
#[test]
fn interactive_write_tool_off_a_terminal_is_a_config_error() {
    let server = MockServer::start();
    let lookup = server.mock(|when, then| {
        when.method(GET).path("/v1/tools/create_campaign");
        then.status(200).json_body(listing_entry(false));
    });
    let post = server.mock(|when, then| {
        when.method(POST).path("/v1/tools/create_campaign");
        then.status(200)
            .json_body(json!({"content": [], "isError": false}));
    });
    let home = tempfile::tempdir().unwrap(); // hermetic: no real ~/.config
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_appcues"))
        .args(["-i", "tools", "call", "create_campaign", "--attr", "name=x"])
        .env_clear()
        .env("HOME", home.path())
        .env("APPCUES_API_KEY", "test-key")
        .env("APPCUES_API_SECRET", "test-secret")
        .env("APPCUES_ACCOUNT_ID", "acct1")
        .env("APPCUES_BASE_URL", server.base_url())
        .env("APPCUES_TOOLS_BASE_URL", server.base_url())
        .stdin(std::process::Stdio::null())
        .output()
        .expect("binary should run");
    assert_eq!(out.status.code(), Some(3), "config error exit");
    let stderr = String::from_utf8_lossy(&out.stderr);
    let line: Value = serde_json::from_str(stderr.trim()).expect("one JSON line on stderr");
    assert_eq!(line["type"], "config");
    assert_eq!(line["exit_code"], 3);
    lookup.assert();
    post.assert_hits(0); // refused before mutating
}

#[test]
fn interactive_read_only_tool_needs_no_confirmation() {
    let server = MockServer::start();
    let lookup = server.mock(|when, then| {
        when.method(GET).path("/v1/tools/list_campaigns");
        then.status(200).json_body(listing_entry(true));
    });
    let post = server.mock(|when, then| {
        when.method(POST).path("/v1/tools/list_campaigns");
        then.status(200).json_body(json!({
            "content": [{"type": "text", "text": "ok"}],
            "isError": false
        }));
    });
    let mut ctx = common::ctx(&server);
    ctx.interactive = true; // non-tty stdin, but readOnlyHint short-circuits confirm
    let out = call(&ctx, "list_campaigns", &call_opts()).unwrap();
    assert_eq!(out, "ok");
    lookup.assert();
    post.assert();
}

#[test]
fn non_interactive_call_skips_the_pre_check_lookup() {
    let server = MockServer::start();
    let lookup = server.mock(|when, then| {
        when.method(GET).path("/v1/tools/create_campaign");
        then.status(200).json_body(listing_entry(false));
    });
    let post = server.mock(|when, then| {
        when.method(POST).path("/v1/tools/create_campaign");
        then.status(200).json_body(json!({
            "content": [{"type": "text", "text": "created"}],
            "isError": false
        }));
    });
    let out = call(&common::ctx(&server), "create_campaign", &call_opts()).unwrap();
    assert_eq!(out, "created");
    lookup.assert_hits(0);
    post.assert();
}

#[test]
fn call_passes_input_arguments_through() {
    let server = MockServer::start();
    let m = server.mock(|when, then| {
        when.method(POST)
            .path("/v1/tools/create_campaign")
            .json_body(json!({"arguments": {"name": "Feature X", "priority": 2}}));
        then.status(200).json_body(json!({
            "content": [{"type": "text", "text": "created"}],
            "isError": false
        }));
    });
    let mut opts = call_opts();
    opts.input = Some(r#"{"name":"Feature X","priority":2}"#);
    call(&common::ctx(&server), "create_campaign", &opts).unwrap();
    m.assert();
}

#[test]
fn dry_run_call_makes_zero_requests() {
    let server = MockServer::start();
    let m = server.mock(|when, then| {
        when.method(POST).path_matches(Regex::new(".*").unwrap());
        then.status(200);
    });
    let mut ctx = common::dry_run_ctx(&server);
    ctx.interactive = true; // dry-run returns before the pre-check and confirm
    let out = call(&ctx, "create_campaign", &call_opts()).unwrap();
    assert!(out.contains("POST") && out.contains("/v1/tools/create_campaign"));
    m.assert_hits(0);
}

#[test]
fn tools_on_env_without_endpoint_is_a_config_error() {
    let server = MockServer::start();
    let mut ctx = common::ctx(&server);
    ctx.tools_client = None; // no tools endpoint configured
    let err = list(&ctx, false).unwrap_err();
    assert!(
        err.downcast_ref::<appcues::config::ConfigError>().is_some(),
        "should classify as config (exit 3)"
    );
}

#[test]
fn call_error_status_maps_to_api_error() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(POST).path("/v1/tools/list_campaigns");
        then.status(400).json_body(json!({
            "error": "invalid_arguments",
            "error_description": "name is required"
        }));
    });
    let err = call(&common::ctx(&server), "list_campaigns", &call_opts()).unwrap_err();
    let api = err
        .downcast_ref::<appcues::client::ApiError>()
        .expect("should be ApiError");
    assert_eq!(api.status, 400);
    assert_eq!(api.body.as_ref().unwrap()["error"], "invalid_arguments");
}
