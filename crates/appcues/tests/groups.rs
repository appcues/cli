mod common;
use appcues::commands::{groups, parse_attrs};
use httpmock::Method::PATCH;
use httpmock::prelude::*;
use serde_json::json;

#[test]
fn get_reads_group_profile() {
    let server = MockServer::start();
    let m = server.mock(|when, then| {
        when.method(GET)
            .path("/v2/accounts/acct1/groups/g1/profile")
            .header("authorization", common::AUTH);
        then.status(200)
            .json_body(json!({"group_id": "g1", "company": "burger appcues"}));
    });
    let out = groups::get(&common::ctx(&server), "g1").unwrap();
    m.assert();
    assert!(out.contains("company : burger appcues"));
}

#[test]
fn update_patches_attributes() {
    let server = MockServer::start();
    let m = server.mock(|when, then| {
        when.method(PATCH)
            .path("/v2/accounts/acct1/groups/g1/profile")
            .json_body(json!({"tier": "enterprise"}));
        then.status(200).json_body(json!({"group_id": "g1"}));
    });
    groups::update(
        &common::ctx(&server),
        "g1",
        parse_attrs(&["tier=enterprise".into()]).unwrap(),
    )
    .unwrap();
    m.assert();
}

#[test]
fn add_users_posts_user_id_list() {
    let server = MockServer::start();
    let m = server.mock(|when, then| {
        when.method(POST)
            .path("/v2/accounts/acct1/groups/g1/users")
            .json_body(json!({"user_ids": ["u1", "u2"]}));
        then.status(204);
    });
    let out = groups::add_users(&common::ctx(&server), "g1", &["u1".into(), "u2".into()]).unwrap();
    m.assert();
    assert!(out.contains("Associated 2 user(s) with group g1"));
}
