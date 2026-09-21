use crate::client::Client;
use crate::output::Format;
use anyhow::{Context, Result, bail};
use serde_json::Value;

pub mod analytics;
pub mod checklists;
pub mod composite;
pub mod experiences;
pub mod flows;
pub mod groups;
pub mod jobs;
pub mod profiles;
pub mod screenshots;
pub mod segments;
pub mod tags;
pub mod tools;
pub mod users;

pub struct Ctx {
    pub client: Client,
    /// Client for the tools routes (separate host, `appcues-api-key`
    /// auth). None when the environment has no tools endpoint.
    pub tools_client: Option<Client>,
    pub account_id: String,
    pub format: Format,
    pub dry_run: bool,
    /// Prompt y/N before destructive ops (`-i` flag, profile field, or
    /// APPCUES_INTERACTIVE). Off by default: destructive ops just run.
    pub interactive: bool,
}

impl Ctx {
    pub fn path(&self, rest: &str) -> String {
        format!("/v2/accounts/{}/{rest}", self.account_id)
    }

    /// The tools client, or a ConfigError (exit 3) when no tools endpoint
    /// is configured.
    pub fn tools(&self) -> Result<&Client> {
        self.tools_client.as_ref().ok_or_else(|| {
            crate::config::ConfigError(
                "no tools endpoint for this environment; set tools_base_url in the profile \
                 or APPCUES_TOOLS_BASE_URL"
                    .into(),
            )
            .into()
        })
    }

    /// Some(rendered preview) when --dry-run is on: the request that would
    /// have been sent. Callers return it instead of calling the API.
    pub fn dry_run_output(&self, method: &str, path: &str, body: Option<&Value>) -> Option<String> {
        if !self.dry_run {
            return None;
        }
        Some(match self.format {
            Format::Json => serde_json::to_string_pretty(&serde_json::json!({
                "dry_run": true, "method": method, "path": path, "body": body,
            }))
            .expect("serializing Value never fails"),
            Format::Table => match body {
                Some(b) => format!("DRY RUN: {method} {path} body: {b}"),
                None => format!("DRY RUN: {method} {path}"),
            },
        })
    }

    /// Dry-run preview for composites that would send several requests:
    /// one JSON array in json mode (so the output stays a single
    /// document), one line per request in table mode.
    pub fn dry_run_outputs(&self, requests: &[(&str, &str, &Value)]) -> Option<String> {
        if !self.dry_run {
            return None;
        }
        Some(match self.format {
            Format::Json => {
                let items: Vec<Value> = requests
                    .iter()
                    .map(|(method, path, body)| {
                        serde_json::json!({"dry_run": true, "method": method, "path": path, "body": body})
                    })
                    .collect();
                serde_json::to_string_pretty(&items).expect("serializing Value never fails")
            }
            Format::Table => requests
                .iter()
                .map(|(method, path, body)| format!("DRY RUN: {method} {path} body: {body}"))
                .collect::<Vec<_>>()
                .join("\n"),
        })
    }
}

/// One informational JSON line to stderr when a response carried
/// rate-limit or truncation headers, so agents can self-regulate:
/// {"rate_limit":{...},"rows_truncated":true}, each key present only
/// when its headers were. Stdout stays data-only; legacy routes send
/// none of these headers, so this prints nothing for them.
pub fn emit_meta(res: &crate::client::Response) {
    let mut line = serde_json::Map::new();
    if let Some(rl) = &res.rate_limit {
        line.insert("rate_limit".to_string(), rl.clone());
    }
    if res.rows_truncated {
        line.insert("rows_truncated".to_string(), true.into());
    }
    if !line.is_empty() {
        eprintln!("{}", Value::Object(line));
    }
}

/// Stream a download into `path` via a temp sibling renamed on success,
/// so a failed download never truncates or corrupts an existing file.
pub fn save_download(
    path: &std::path::Path,
    fetch: impl FnOnce(&mut std::fs::File) -> Result<u64>,
) -> Result<u64> {
    let tmp = std::path::PathBuf::from(format!("{}.part", path.display()));
    let mut file = std::fs::File::create(&tmp)
        .with_context(|| format!("failed to create {}", tmp.display()))?;
    let bytes = match fetch(&mut file) {
        Ok(bytes) => bytes,
        Err(e) => {
            drop(file);
            let _ = std::fs::remove_file(&tmp);
            return Err(e);
        }
    };
    drop(file);
    std::fs::rename(&tmp, path)
        .with_context(|| format!("failed to move {} to {}", tmp.display(), path.display()))?;
    Ok(bytes)
}

/// List endpoints may return a bare array or an object wrapping one array.
pub fn as_list(v: Value) -> Vec<Value> {
    match v {
        Value::Array(a) => a,
        Value::Object(map) => map
            .into_iter()
            .find_map(|(_, v)| match v {
                Value::Array(a) => Some(a),
                _ => None,
            })
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

/// "key=value" pairs; values parsed as JSON (3 → number, true → bool), falling back to string.
pub fn parse_attrs(pairs: &[String]) -> Result<serde_json::Map<String, Value>> {
    let mut map = serde_json::Map::new();
    for pair in pairs {
        let Some((key, value)) = pair.split_once('=') else {
            bail!("invalid attribute '{pair}': expected key=value");
        };
        map.insert(
            key.to_string(),
            serde_json::from_str(value).unwrap_or(Value::String(value.to_string())),
        );
    }
    Ok(map)
}

/// Destructive-op gate. Non-interactive (the default) runs straight
/// through: the CLI is agent-first, and safety is delegated to API key
/// permissions. Interactive mode (`-i`, profile `interactive = true`, or
/// APPCUES_INTERACTIVE) prompts y/N on a terminal, and is a ConfigError
/// off one — there is nobody to answer.
pub fn confirm(prompt: &str, interactive: bool) -> Result<()> {
    use std::io::IsTerminal;
    confirm_with(
        prompt,
        interactive,
        std::io::stdin().is_terminal(),
        &mut std::io::stdin().lock(),
    )
}

fn confirm_with(
    prompt: &str,
    interactive: bool,
    is_tty: bool,
    input: &mut dyn std::io::BufRead,
) -> Result<()> {
    if !interactive {
        return Ok(());
    }
    if !is_tty {
        return Err(crate::config::ConfigError(
            "interactive confirmation is on but stdin is not a terminal; \
             drop -i or set APPCUES_INTERACTIVE=false"
                .into(),
        )
        .into());
    }
    eprint!("{prompt} [y/N] ");
    let mut line = String::new();
    input.read_line(&mut line)?;
    if line.trim().eq_ignore_ascii_case("y") {
        Ok(())
    } else {
        bail!("aborted")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_attrs_types_values_as_json_with_string_fallback() {
        let attrs = parse_attrs(&[
            "count=3".to_string(),
            "active=true".to_string(),
            "name=Jane Doe".to_string(),
        ])
        .unwrap();
        assert_eq!(attrs["count"], json!(3));
        assert_eq!(attrs["active"], json!(true));
        assert_eq!(attrs["name"], json!("Jane Doe"));
    }

    #[test]
    fn parse_attrs_rejects_missing_equals() {
        assert!(parse_attrs(&["nonsense".to_string()]).is_err());
    }

    #[test]
    fn as_list_handles_bare_and_wrapped_arrays() {
        assert_eq!(as_list(json!([1, 2])).len(), 2);
        assert_eq!(as_list(json!({"flows": [1, 2, 3]})).len(), 3);
        assert_eq!(as_list(json!("scalar")).len(), 0);
    }

    #[test]
    fn non_interactive_passes_without_prompting() {
        let mut input = "".as_bytes();
        assert!(confirm_with("Delete?", false, false, &mut input).is_ok());
        assert!(confirm_with("Delete?", false, true, &mut input).is_ok());
    }

    #[test]
    fn interactive_without_tty_is_a_config_error() {
        let mut input = "".as_bytes();
        let err = confirm_with("Delete?", true, false, &mut input).unwrap_err();
        assert!(err.downcast_ref::<crate::config::ConfigError>().is_some());
        assert!(err.to_string().contains("APPCUES_INTERACTIVE"));
    }

    #[test]
    fn interactive_tty_accepts_y() {
        let mut input = "y\n".as_bytes();
        assert!(confirm_with("Delete?", true, true, &mut input).is_ok());
    }

    #[test]
    fn interactive_tty_rejects_n() {
        let mut input = "n\n".as_bytes();
        let err = confirm_with("Delete?", true, true, &mut input).unwrap_err();
        assert_eq!(err.to_string(), "aborted");
    }

    #[test]
    fn interactive_tty_rejects_eof() {
        let mut input = "".as_bytes();
        let err = confirm_with("Delete?", true, true, &mut input).unwrap_err();
        assert_eq!(err.to_string(), "aborted");
    }

    #[test]
    fn ctx_path_prepends_account() {
        let ctx = Ctx {
            client: crate::client::Client::new("http://localhost:9", "test-key", "test-secret"),
            tools_client: None,
            account_id: "acct1".to_string(),
            format: crate::output::Format::Json,
            dry_run: false,
            interactive: false,
        };
        assert_eq!(ctx.path("flows/f1"), "/v2/accounts/acct1/flows/f1");
    }

    #[test]
    fn tools_without_endpoint_is_a_config_error() {
        let ctx = Ctx {
            client: crate::client::Client::new("http://localhost:9", "test-key", "test-secret"),
            tools_client: None,
            account_id: "acct1".to_string(),
            format: crate::output::Format::Json,
            dry_run: false,
            interactive: false,
        };
        let err = match ctx.tools() {
            Ok(_) => panic!("expected a ConfigError"),
            Err(e) => e,
        };
        assert!(
            err.downcast_ref::<crate::config::ConfigError>().is_some(),
            "should classify as config (exit 3)"
        );
        assert!(err.to_string().contains("tools_base_url"));
    }
}
