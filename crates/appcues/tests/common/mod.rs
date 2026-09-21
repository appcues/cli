// Each tests/*.rs is its own crate including this module, so not every
// binary uses every item — silence per-binary dead_code warnings.
#![allow(dead_code)]

use appcues::client::Client;
use appcues::commands::Ctx;
use appcues::output::Format;
use httpmock::MockServer;

// base64("test-key:test-secret")
pub const AUTH: &str = "Basic dGVzdC1rZXk6dGVzdC1zZWNyZXQ=";
// The same credential without the Basic prefix — the tools routes'
// appcues-api-key header value.
pub const KEY_AUTH: &str = "dGVzdC1rZXk6dGVzdC1zZWNyZXQ=";

pub fn ctx(server: &MockServer) -> Ctx {
    Ctx {
        client: Client::new(&server.base_url(), "test-key", "test-secret"),
        tools_client: Some(Client::with_header_auth(
            &server.base_url(),
            "appcues-api-key",
            appcues::client::api_key_credential("test-key", "test-secret"),
        )),
        account_id: "acct1".to_string(),
        format: Format::Table,
        dry_run: false,
        interactive: false,
    }
}

pub fn dry_run_ctx(server: &MockServer) -> Ctx {
    let mut c = ctx(server);
    c.dry_run = true;
    c
}
