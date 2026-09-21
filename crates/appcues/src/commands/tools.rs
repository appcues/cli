use super::{Ctx, confirm, emit_meta, parse_attrs};
use crate::output::{Format, render_item, render_list};
use anyhow::{Context, Result, anyhow, bail};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use serde_json::{Value, json};
use std::path::Path;

/// A tool ran and reported failure: HTTP 200 with `isError: true`. The
/// tool owns the message; main.rs downcasts this to exit 4, type "tool",
/// with the full envelope embedded in the one-line JSON error. Exit 0 on
/// isError would break every skill's error-branching table.
#[derive(Debug)]
pub struct ToolError {
    pub tool: String,
    pub message: String,
    pub envelope: Value,
}

impl std::fmt::Display for ToolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "tool {} failed: {}", self.tool, self.message)
    }
}
impl std::error::Error for ToolError {}

/// Flags of `appcues tools call`. Exclusivity of input/input-file/attr is
/// enforced by clap (`conflicts_with`), so at most one is set here.
pub struct CallOpts<'a> {
    /// Arguments as an inline JSON object, or "-" for stdin.
    pub input: Option<&'a str>,
    /// Arguments read from a JSON file.
    pub input_file: Option<&'a Path>,
    /// key=value pairs, JSON-typed like every other --attr flag.
    pub attrs: &'a [String],
    /// Print the whole envelope instead of extracted data.
    pub raw: bool,
    /// Directory for files the tool returns (default: current dir).
    pub out: Option<&'a Path>,
}

/// List the catalog. The default asks for the summary view (name, role,
/// description only, a fraction of the full payload) so agents can
/// search-then-load via `describe`; `full` fetches complete entries with
/// inputSchema and annotations. A server without the summary view treats
/// the unknown parameter as the full listing, so nothing breaks either way.
pub fn list(ctx: &Ctx, full: bool) -> Result<String> {
    let path = if full {
        "/v1/tools"
    } else {
        "/v1/tools?view=summary"
    };
    let v = ctx.tools()?.get(path)?;
    let tools = match v.get("tools") {
        Some(Value::Array(a)) => a.clone(),
        _ => Vec::new(),
    };
    match ctx.format {
        Format::Json => {
            Ok(serde_json::to_string_pretty(&tools).expect("serializing Value never fails"))
        }
        Format::Table => {
            let rows: Vec<Value> = tools
                .iter()
                .map(|t| {
                    let mut row = json!({
                        "name": t.get("name").cloned().unwrap_or_default(),
                        "role": t.get("role").cloned().unwrap_or_default(),
                        "description": truncate(
                            t.get("description").and_then(Value::as_str).unwrap_or(""),
                        ),
                    });
                    if full {
                        row["title"] = t.pointer("/annotations/title").cloned().unwrap_or_default();
                    }
                    row
                })
                .collect();
            let columns: &[&str] = if full {
                &["name", "role", "title", "description"]
            } else {
                &["name", "role", "description"]
            };
            Ok(render_list(&rows, ctx.format, columns))
        }
    }
}

/// Cap table descriptions so the schema-bearing entries stay one line;
/// `-o json` always carries the full text.
fn truncate(s: &str) -> String {
    if s.chars().count() <= 60 {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(59).collect::<String>())
    }
}

pub fn describe(ctx: &Ctx, name: &str) -> Result<String> {
    let v = ctx.tools()?.get(&format!("/v1/tools/{name}"))?;
    Ok(render_item(&v, ctx.format))
}

pub fn call(ctx: &Ctx, name: &str, opts: &CallOpts) -> Result<String> {
    let arguments = build_arguments(opts, &mut std::io::stdin().lock())?;
    match invoke(ctx, name, arguments)? {
        Invoked::DryRun(msg) => Ok(msg),
        Invoked::Envelope(envelope) => render_envelope(ctx, name, envelope, opts),
    }
}

/// What `invoke` produced: the dry-run preview, or the tool's raw result
/// envelope (`content`, `isError`, ...), which each interface renders its
/// own way (the CLI extracts data; the MCP server forwards content blocks).
pub enum Invoked {
    DryRun(String),
    Envelope(Value),
}

/// POST `arguments` to the tool, honoring --dry-run and the interactive
/// confirmation gate. Shared by `appcues tools call` and the MCP server.
pub fn invoke(ctx: &Ctx, name: &str, arguments: Value) -> Result<Invoked> {
    let body = json!({ "arguments": arguments });
    let path = format!("/v1/tools/{name}");
    if let Some(msg) = ctx.dry_run_output("POST", &path, Some(&body)) {
        return Ok(Invoked::DryRun(msg));
    }
    let client = ctx.tools()?;
    // Interactive mode only: one extra GET to learn the tool's
    // readOnlyHint, then a y/N prompt before anything that mutates.
    // The non-interactive default makes zero extra requests.
    if ctx.interactive {
        let entry = client.get(&path)?;
        let read_only = entry
            .pointer("/annotations/readOnlyHint")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if !read_only {
            confirm(&format!("Run {name}? It modifies account data."), true)?;
        }
    }
    let res = client.post_with_meta(&path, Some(&body))?;
    emit_meta(&res);
    Ok(Invoked::Envelope(res.value))
}

/// Build the `arguments` object from whichever input flag was given;
/// none of them means an empty object (tools without required inputs).
fn build_arguments(opts: &CallOpts, stdin: &mut dyn std::io::Read) -> Result<Value> {
    let text = match (opts.input, opts.input_file) {
        (Some("-"), _) => {
            let mut s = String::new();
            stdin
                .read_to_string(&mut s)
                .context("failed to read arguments from stdin")?;
            s
        }
        (Some(s), _) => s.to_string(),
        (None, Some(p)) => std::fs::read_to_string(p)
            .with_context(|| format!("cannot read input file {}", p.display()))?,
        (None, None) => {
            if !opts.attrs.is_empty() {
                return Ok(Value::Object(parse_attrs(opts.attrs)?));
            }
            return Ok(json!({}));
        }
    };
    let v: Value =
        serde_json::from_str(&text).map_err(|e| anyhow!("arguments are not valid JSON: {e}"))?;
    if !v.is_object() {
        bail!("arguments must be a JSON object");
    }
    Ok(v)
}

fn render_envelope(ctx: &Ctx, name: &str, envelope: Value, opts: &CallOpts) -> Result<String> {
    if envelope
        .get("isError")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return Err(ToolError {
            tool: name.to_string(),
            message: joined_text(&envelope),
            envelope,
        }
        .into());
    }
    let images = save_images(name, &envelope, opts.out.unwrap_or(Path::new(".")))?;
    let rendered = if opts.raw {
        serde_json::to_string_pretty(&envelope).expect("serializing Value never fails")
    } else if let Some(data) = envelope
        .get("data")
        .and_then(Value::as_array)
        .filter(|a| !a.is_empty())
    {
        render_list(data, ctx.format, &["type", "id", "name"])
    } else if let Some(parsed) = single_json_text(&envelope) {
        render_item(&parsed, ctx.format)
    } else {
        joined_text(&envelope)
    };
    if images.is_empty() {
        return Ok(rendered);
    }
    match ctx.format {
        Format::Table => {
            let mut lines: Vec<String> = if rendered.is_empty() {
                Vec::new()
            } else {
                vec![rendered]
            };
            lines.extend(images.iter().map(|p| format!("Saved {p}")));
            Ok(lines.join("\n"))
        }
        // Keep stdout parseable in json mode: saved paths go to stderr as
        // one JSON meta line, like emit_meta does for rate limits.
        Format::Json => {
            eprintln!("{}", json!({ "images": images }));
            Ok(rendered)
        }
    }
}

/// All text content items joined with newlines; empty when there are none.
fn joined_text(envelope: &Value) -> String {
    envelope
        .get("content")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter(|i| i.get("type").and_then(Value::as_str) == Some("text"))
                .filter_map(|i| i.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}

/// When the content is exactly one text item holding a JSON object or
/// array, the parsed value — so agents don't double-parse JSON-in-text.
fn single_json_text(envelope: &Value) -> Option<Value> {
    let items = envelope.get("content").and_then(Value::as_array)?;
    let texts: Vec<&str> = items
        .iter()
        .filter(|i| i.get("type").and_then(Value::as_str) == Some("text"))
        .filter_map(|i| i.get("text").and_then(Value::as_str))
        .collect();
    match texts.as_slice() {
        [one] => serde_json::from_str::<Value>(one)
            .ok()
            .filter(|v| v.is_object() || v.is_array()),
        _ => None,
    }
}

/// Write each image content item to `<dir>/<tool>-<timestamp>[-<i>].png`
/// (the contract's only image type is png) and return the paths.
fn save_images(name: &str, envelope: &Value, dir: &Path) -> Result<Vec<String>> {
    let Some(items) = envelope.get("content").and_then(Value::as_array) else {
        return Ok(Vec::new());
    };
    let images: Vec<&Value> = items
        .iter()
        .filter(|i| i.get("type").and_then(Value::as_str) == Some("image"))
        .collect();
    // Millisecond precision so repeated calls don't silently overwrite
    // an earlier file (fs::write replaces without notice).
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock before 1970")
        .as_millis();
    let mut paths = Vec::new();
    for (i, item) in images.iter().enumerate() {
        let data = item.get("data").and_then(Value::as_str).unwrap_or("");
        let bytes = B64
            .decode(data)
            .with_context(|| format!("tool {name} returned invalid base64 image data"))?;
        let suffix = if images.len() > 1 {
            format!("-{i}")
        } else {
            String::new()
        };
        let path = dir.join(format!("{name}-{ts}{suffix}.png"));
        std::fs::write(&path, bytes)
            .with_context(|| format!("failed to write {}", path.display()))?;
        paths.push(path.display().to_string());
    }
    Ok(paths)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts<'a>(input: Option<&'a str>, attrs: &'a [String]) -> CallOpts<'a> {
        CallOpts {
            input,
            input_file: None,
            attrs,
            raw: false,
            out: None,
        }
    }

    #[test]
    fn arguments_parse_inline_json() {
        let v = build_arguments(&opts(Some(r#"{"name":"x"}"#), &[]), &mut "".as_bytes()).unwrap();
        assert_eq!(v["name"], "x");
    }

    #[test]
    fn arguments_read_stdin_when_dash() {
        let v = build_arguments(&opts(Some("-"), &[]), &mut r#"{"n":1}"#.as_bytes()).unwrap();
        assert_eq!(v["n"], 1);
    }

    #[test]
    fn arguments_from_attrs_are_json_typed() {
        let attrs = vec!["count=3".to_string(), "name=Launch".to_string()];
        let v = build_arguments(&opts(None, &attrs), &mut "".as_bytes()).unwrap();
        assert_eq!(v["count"], 3);
        assert_eq!(v["name"], "Launch");
    }

    #[test]
    fn no_input_means_empty_object() {
        let v = build_arguments(&opts(None, &[]), &mut "".as_bytes()).unwrap();
        assert_eq!(v, serde_json::json!({}));
    }

    #[test]
    fn non_object_input_is_rejected() {
        let err = build_arguments(&opts(Some("[1,2]"), &[]), &mut "".as_bytes()).unwrap_err();
        assert!(err.to_string().contains("JSON object"));
    }

    #[test]
    fn invalid_json_input_is_rejected() {
        let err = build_arguments(&opts(Some("{nope"), &[]), &mut "".as_bytes()).unwrap_err();
        assert!(err.to_string().contains("not valid JSON"));
    }

    #[test]
    fn single_json_text_parses_objects_but_not_prose() {
        let env = |text: &str| serde_json::json!({"content":[{"type":"text","text": text}]});
        assert_eq!(
            single_json_text(&env(r#"{"a":1}"#)).unwrap()["a"],
            serde_json::json!(1)
        );
        assert!(single_json_text(&env("plain prose")).is_none());
        assert!(single_json_text(&env("42")).is_none()); // scalars stay verbatim
    }

    #[test]
    fn joined_text_skips_non_text_items() {
        let env = serde_json::json!({"content":[
            {"type":"text","text":"a"},
            {"type":"image","data":"aGk="},
            {"type":"text","text":"b"},
        ]});
        assert_eq!(joined_text(&env), "a\nb");
    }

    #[test]
    fn truncate_caps_long_descriptions() {
        assert_eq!(truncate("short"), "short");
        let long = "x".repeat(80);
        let out = truncate(&long);
        assert_eq!(out.chars().count(), 60);
        assert!(out.ends_with('…'));
    }
}
