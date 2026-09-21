mod common;
use appcues::commands::experiences::{self, ExperienceType};
use httpmock::prelude::*;
use serde_json::json;

#[test]
fn list_hits_type_route_with_auth_and_renders_table() {
    let server = MockServer::start();
    let m = server.mock(|when, then| {
        when.method(GET)
            .path("/v2/accounts/acct1/pins")
            .header("authorization", common::AUTH);
        then.status(200).json_body(json!([
            {"id": "p1", "name": "Help pin", "published": true, "updated_at": "2026-01-01T00:00:00Z"}
        ]));
    });
    let out = experiences::list(&common::ctx(&server), ExperienceType::Pins).unwrap();
    m.assert();
    assert!(out.contains("Help pin") && out.contains("true"));
}

#[test]
fn flows_v2_type_maps_to_kebab_case_route() {
    let server = MockServer::start();
    let m = server.mock(|when, then| {
        when.method(GET)
            .path("/v2/accounts/acct1/flows-v2")
            .header("authorization", common::AUTH);
        then.status(200).json_body(json!([]));
    });
    experiences::list(&common::ctx(&server), ExperienceType::FlowsV2).unwrap();
    m.assert();
}

#[test]
fn get_renders_single_experience() {
    let server = MockServer::start();
    let m = server.mock(|when, then| {
        when.method(GET)
            .path("/v2/accounts/acct1/banners/b1")
            .header("authorization", common::AUTH);
        then.status(200)
            .json_body(json!({"id": "b1", "name": "Promo banner", "published": false}));
    });
    let out = experiences::get(&common::ctx(&server), ExperienceType::Banners, "b1").unwrap();
    m.assert();
    assert!(out.contains("Promo banner"), "got: {out}");
}

#[test]
fn publish_posts_to_each_type_route() {
    let cases = [
        (ExperienceType::Pins, "pins"),
        (ExperienceType::Mobile, "mobile"),
        (ExperienceType::Launchpads, "launchpads"),
        (ExperienceType::Banners, "banners"),
        (ExperienceType::FlowsV2, "flows-v2"),
        (ExperienceType::Embeds, "embeds"),
        (ExperienceType::Nps, "nps"),
    ];
    for (kind, segment) in cases {
        let server = MockServer::start();
        let m = server.mock(|when, then| {
            when.method(POST)
                .path(format!("/v2/accounts/acct1/{segment}/e1/publish"))
                .header("authorization", common::AUTH);
            then.status(204);
        });
        let out = experiences::publish(&common::ctx(&server), kind, "e1").unwrap();
        m.assert();
        assert!(
            out.contains(&format!("Published {segment} e1")),
            "got: {out}"
        );
    }
}

#[test]
fn unpublish_posts_to_unpublish_path() {
    let server = MockServer::start();
    let m = server.mock(|when, then| {
        when.method(POST)
            .path("/v2/accounts/acct1/banners/b1/unpublish")
            .header("authorization", common::AUTH);
        then.status(204);
    });
    let out = experiences::unpublish(&common::ctx(&server), ExperienceType::Banners, "b1").unwrap();
    m.assert();
    assert!(out.contains("Unpublished banners b1"), "got: {out}");
}

#[test]
fn dry_run_publish_previews_without_any_request() {
    // No mocks registered: any request to the mock server would 404 and fail.
    let server = MockServer::start();
    let out =
        experiences::publish(&common::dry_run_ctx(&server), ExperienceType::Pins, "p1").unwrap();
    assert!(
        out.contains("DRY RUN: POST /v2/accounts/acct1/pins/p1/publish"),
        "got: {out}"
    );
}

#[test]
fn api_error_propagates() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(GET).path("/v2/accounts/acct1/mobile/nope");
        then.status(404).json_body(
            json!({"error": true, "status": 404, "title": "Not Found", "detail": "experience not found"}),
        );
    });
    let err = experiences::get(&common::ctx(&server), ExperienceType::Mobile, "nope").unwrap_err();
    assert!(err.to_string().contains("experience not found"));
}
