mod common;

use appcues::mcp::AppcuesMcp;
use appcues::output::Format;
use httpmock::Method::PATCH;
use httpmock::prelude::*;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock};
use serde_json::{Value, json};

/// The text of the one content block a tool result carries.
fn text(result: &CallToolResult) -> String {
    match result.content.as_slice() {
        [ContentBlock::Text(t)] => t.text.clone(),
        other => panic!("expected one text block, got {other:?}"),
    }
}

fn error_json(result: &CallToolResult) -> Value {
    assert_eq!(result.is_error, Some(true), "expected an isError result");
    serde_json::from_str(&text(result)).expect("error text is one JSON object")
}

#[test]
fn catalog_is_semantic_tools_with_typed_schemas_and_hints() {
    let tools = AppcuesMcp::tool_router().list_all();
    let names: Vec<&str> = tools.iter().map(|t| t.name.as_ref()).collect();
    for expected in [
        "list_flows",
        "publish_flow",
        "list_experiences",
        "create_segment",
        "delete_segment",
        "update_user",
        "run_analytics_query",
        "start_analytics_export",
        "download_export_job",
        "call_account_tool",
        "verify_credentials",
    ] {
        assert!(names.contains(&expected), "missing tool {expected}");
    }
    assert!(
        !names
            .iter()
            .any(|n| n.contains("cli") || n.contains("run_command"))
    );

    let publish = tools.iter().find(|t| t.name == "publish_flow").unwrap();
    assert_eq!(publish.input_schema["required"], json!(["flow_id"]));
    assert!(publish.description.as_deref().unwrap().contains("Publish"));

    let list = tools.iter().find(|t| t.name == "list_flows").unwrap();
    assert_eq!(
        list.annotations.as_ref().unwrap().read_only_hint,
        Some(true)
    );
    let delete = tools.iter().find(|t| t.name == "delete_segment").unwrap();
    assert_eq!(
        delete.annotations.as_ref().unwrap().destructive_hint,
        Some(true)
    );
    // Publishing and updating overwrite state: never claim additive-only.
    // Downloads write local files: never claim read-only.
    for name in [
        "publish_flow",
        "unpublish_experience",
        "update_user",
        "update_segment",
    ] {
        let t = tools.iter().find(|t| t.name == name).unwrap();
        assert_ne!(
            t.annotations.as_ref().unwrap().destructive_hint,
            Some(false),
            "{name}"
        );
    }
    for name in ["download_screenshots", "download_export_job"] {
        let t = tools.iter().find(|t| t.name == name).unwrap();
        assert_eq!(
            t.annotations.as_ref().unwrap().read_only_hint,
            Some(false),
            "{name}"
        );
    }

    // The experience type is a closed enum in kebab-case, same as the CLI.
    let exp = tools.iter().find(|t| t.name == "list_experiences").unwrap();
    let schema = serde_json::to_value(&*exp.input_schema).unwrap();
    assert!(
        schema.to_string().contains("flows-v2"),
        "experience_type enum missing: {schema}"
    );
}

#[tokio::test]
async fn list_flows_returns_the_api_json() {
    let server = MockServer::start();
    let m = server.mock(|when, then| {
        when.method(GET)
            .path("/v2/accounts/acct1/flows")
            .header("authorization", common::AUTH);
        then.status(200)
            .json_body(json!([{"id": "f1", "name": "Welcome", "published": true}]));
    });
    let out = AppcuesMcp::new(common::ctx(&server))
        .list_flows()
        .await
        .unwrap();
    m.assert();
    assert_eq!(out.is_error, Some(false));
    let v: Value = serde_json::from_str(&text(&out)).unwrap();
    assert_eq!(v[0]["name"], "Welcome");
}

#[tokio::test]
async fn publish_flow_posts_and_confirms() {
    let server = MockServer::start();
    let m = server.mock(|when, then| {
        when.method(POST)
            .path("/v2/accounts/acct1/flows/f1/publish");
        then.status(204);
    });
    let out = AppcuesMcp::new(common::ctx(&server))
        .publish_flow(Parameters(appcues::mcp::FlowId {
            flow_id: "f1".into(),
        }))
        .await
        .unwrap();
    m.assert();
    assert_eq!(out.is_error, Some(false));
    assert!(text(&out).contains("Published flow f1"));
}

#[tokio::test]
async fn api_404_becomes_a_structured_tool_error() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(GET).path("/v2/accounts/acct1/flows/nope");
        then.status(404).json_body(json!({
            "error": true, "status": 404, "title": "Not Found", "detail": "flow not found"
        }));
    });
    let out = AppcuesMcp::new(common::ctx(&server))
        .get_flow(Parameters(appcues::mcp::FlowId {
            flow_id: "nope".into(),
        }))
        .await
        .unwrap();
    let e = error_json(&out);
    assert_eq!(e["type"], "api");
    assert_eq!(e["status"], 404);
    assert_eq!(e["body"]["detail"], "flow not found");
    assert!(e["message"].as_str().unwrap().contains("flow not found"));
}

#[tokio::test]
async fn auth_failure_is_reported_as_auth_without_credentials() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(GET).path("/v2/accounts/acct1/tags");
        then.status(401).body("unauthorized");
    });
    let out = AppcuesMcp::new(common::ctx(&server))
        .verify_credentials()
        .await
        .unwrap();
    let e = error_json(&out);
    assert_eq!(e["type"], "auth");
    assert!(!text(&out).contains("test-secret"));
}

#[tokio::test]
async fn account_tools_without_an_endpoint_is_a_config_error() {
    let server = MockServer::start();
    let mut ctx = common::ctx(&server);
    ctx.tools_client = None;
    let out = AppcuesMcp::new(ctx)
        .list_account_tools(Parameters(appcues::mcp::ListAccountToolsParams {
            full: None,
        }))
        .await
        .unwrap();
    let e = error_json(&out);
    assert_eq!(e["type"], "config");
    assert!(e["message"].as_str().unwrap().contains("tools_base_url"));
}

#[tokio::test]
async fn call_account_tool_forwards_content_and_is_error() {
    let server = MockServer::start();
    let ok = server.mock(|when, then| {
        when.method(POST)
            .path("/v1/tools/list_campaigns")
            .header("appcues-api-key", common::KEY_AUTH)
            .json_body(json!({"arguments": {"status": "active"}}));
        then.status(200).json_body(json!({
            "content": [
                {"type": "text", "text": "[{\"id\":\"c1\"}]"},
                {"type": "image", "data": "aGk=", "mimeType": "image/png"}
            ],
            "isError": false
        }));
    });
    let failing = server.mock(|when, then| {
        when.method(POST).path("/v1/tools/create_campaign");
        then.status(200).json_body(json!({
            "content": [{"type": "text", "text": "objective_id not found"}],
            "isError": true
        }));
    });
    let mcp = AppcuesMcp::new(common::ctx(&server));

    let out = mcp
        .call_account_tool(Parameters(appcues::mcp::CallAccountToolParams {
            name: "list_campaigns".into(),
            arguments: Some(json!({"status": "active"}).as_object().cloned().unwrap()),
        }))
        .await
        .unwrap();
    ok.assert();
    assert_eq!(out.is_error, Some(false));
    assert_eq!(out.content.len(), 2);
    assert!(matches!(&out.content[0], ContentBlock::Text(t) if t.text.contains("c1")));
    assert!(matches!(&out.content[1], ContentBlock::Image(i) if i.data == "aGk="));

    let out = mcp
        .call_account_tool(Parameters(appcues::mcp::CallAccountToolParams {
            name: "create_campaign".into(),
            arguments: None,
        }))
        .await
        .unwrap();
    failing.assert();
    assert_eq!(out.is_error, Some(true));
    assert!(text(&out).contains("objective_id not found"));
}

#[tokio::test]
async fn update_user_sends_the_attribute_object_with_json_types() {
    let server = MockServer::start();
    let m = server.mock(|when, then| {
        when.method(PATCH)
            .path("/v2/accounts/acct1/users/u1/profile")
            .json_body(json!({"plan": "pro", "seats": 5}));
        then.status(200).json_body(json!({"user_id": "u1"}));
    });
    AppcuesMcp::new(common::ctx(&server))
        .update_user(Parameters(appcues::mcp::UpdateUserParams {
            user_id: "u1".into(),
            attributes: json!({"plan": "pro", "seats": 5})
                .as_object()
                .cloned()
                .unwrap(),
        }))
        .await
        .unwrap();
    m.assert();
}

#[tokio::test]
async fn digest_rejects_days_out_of_range_before_any_request() {
    let server = MockServer::start();
    let any = server.mock(|when, then| {
        when.any_request();
        then.status(200).json_body(json!([]));
    });
    let out = AppcuesMcp::new(common::ctx(&server))
        .flow_performance_digest(Parameters(appcues::mcp::DigestParams { days: Some(91) }))
        .await
        .unwrap();
    any.assert_hits(0);
    let e = error_json(&out);
    assert!(e["message"].as_str().unwrap().contains("between 1 and 90"));
}

#[tokio::test]
async fn interactive_and_table_format_are_forced_off() {
    // stdin is the MCP transport: a profile with interactive = true must
    // not turn destructive tools into a prompt (or a ConfigError off a tty).
    let server = MockServer::start();
    let m = server.mock(|when, then| {
        when.method(DELETE).path("/v2/accounts/acct1/segments/s1");
        then.status(204);
    });
    let list = server.mock(|when, then| {
        when.method(GET).path("/v2/accounts/acct1/tags");
        then.status(200)
            .json_body(json!([{"id": "t1", "name": "x"}]));
    });
    let mut ctx = common::ctx(&server);
    ctx.interactive = true;
    ctx.format = Format::Table;
    let mcp = AppcuesMcp::new(ctx);
    let out = mcp
        .delete_segment(Parameters(appcues::mcp::SegmentId {
            segment_id: "s1".into(),
        }))
        .await
        .unwrap();
    m.assert();
    assert_eq!(out.is_error, Some(false));

    let out = mcp.list_tags().await.unwrap();
    list.assert();
    let v: Value = serde_json::from_str(&text(&out)).expect("JSON, not a table");
    assert_eq!(v[0]["id"], "t1");
}

#[tokio::test]
async fn dry_run_previews_writes_without_sending() {
    let server = MockServer::start();
    let any = server.mock(|when, then| {
        when.any_request();
        then.status(204);
    });
    let out = AppcuesMcp::new(common::dry_run_ctx(&server))
        .publish_flow(Parameters(appcues::mcp::FlowId {
            flow_id: "f1".into(),
        }))
        .await
        .unwrap();
    any.assert_hits(0);
    let v: Value = serde_json::from_str(&text(&out)).unwrap();
    assert_eq!(v["dry_run"], true);
    assert_eq!(v["method"], "POST");
}

#[tokio::test]
async fn download_paths_outside_the_working_directory_are_rejected() {
    let server = MockServer::start();
    let any = server.mock(|when, then| {
        when.any_request();
        then.status(200).body("zip");
    });
    let mcp = AppcuesMcp::new(common::ctx(&server));
    for bad in ["/tmp/evil.zip", "../evil.zip", "ok/../../evil.zip"] {
        let out = mcp
            .download_screenshots(Parameters(appcues::mcp::ScreenshotParams {
                resource_id: "f1".into(),
                out_path: Some(bad.into()),
            }))
            .await
            .unwrap();
        let e = error_json(&out);
        assert!(
            e["message"].as_str().unwrap().contains("out_path"),
            "{bad}: {e}"
        );
    }
    any.assert_hits(0);
}
