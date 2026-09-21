use crate::client::ApiError;
use crate::commands::tools::ToolError;
use crate::config::ConfigError;
use serde_json::Value;

#[derive(Clone, Copy, PartialEq, Eq, Debug, clap::ValueEnum)]
pub enum Format {
    Table,
    Json,
}

fn cell(v: Option<&Value>) -> String {
    match v {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
    }
}

pub fn render_list(items: &[Value], fmt: Format, columns: &[&str]) -> String {
    match fmt {
        Format::Json => serde_json::to_string_pretty(items).expect("serializing Value never fails"),
        Format::Table => {
            let mut table = comfy_table::Table::new();
            table.load_preset(comfy_table::presets::UTF8_BORDERS_ONLY);
            table.set_header(columns.to_vec());
            for item in items {
                table.add_row(columns.iter().map(|c| cell(item.get(*c))));
            }
            table.to_string()
        }
    }
}

pub fn render_item(v: &Value, fmt: Format) -> String {
    match fmt {
        Format::Json => serde_json::to_string_pretty(v).expect("serializing Value never fails"),
        Format::Table => match v.as_object() {
            Some(map) => {
                let width = map.keys().map(String::len).max().unwrap_or(0);
                map.iter()
                    .map(|(k, val)| format!("{:<width$}: {}", k, cell(Some(val))))
                    .collect::<Vec<_>>()
                    .join("\n")
            }
            None => cell(Some(v)),
        },
    }
}

/// (exit code, error type tag, HTTP status if any). Exit code 2 is
/// reserved for clap's own usage errors.
pub fn classify(e: &anyhow::Error) -> (i32, &'static str, Option<u16>) {
    if e.downcast_ref::<ConfigError>().is_some() {
        return (3, "config", None);
    }
    if e.downcast_ref::<ToolError>().is_some() {
        return (4, "tool", None);
    }
    match e.downcast_ref::<ApiError>() {
        Some(a) if a.status == 401 || a.status == 403 => (3, "auth", Some(a.status)),
        Some(a) if a.status == 429 => (5, "rate_limited", Some(a.status)),
        Some(a) if a.status >= 500 => (5, "server", Some(a.status)),
        Some(a) => (4, "api", Some(a.status)),
        None => (1, "unexpected", None),
    }
}

/// The one-line JSON every failure becomes (stderr for the CLI, the tool
/// result text for MCP): `type`, `status`, `message`, `exit_code`, plus
/// the API error `body`/`rate_limit` or the failing tool's envelope.
pub fn render_error(e: &anyhow::Error) -> (i32, String) {
    let (code, kind, status) = classify(e);
    let mut line = serde_json::json!({
        "error": true,
        "type": kind,
        "status": status,
        "message": format!("{e:#}"),
        "exit_code": code,
    });
    if let Some(api) = e.downcast_ref::<ApiError>() {
        if let Some(body) = &api.body {
            line["body"] = body.clone();
        }
        if let Some(rl) = &api.rate_limit {
            line["rate_limit"] = rl.clone();
        }
    }
    if let Some(t) = e.downcast_ref::<ToolError>() {
        line["tool"] = t.tool.clone().into();
        line["body"] = t.envelope.clone();
    }
    (code, line.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn json_format_is_pretty_passthrough() {
        let items = vec![json!({"id": "f1", "name": "Welcome"})];
        let out = render_list(&items, Format::Json, &["id", "name"]);
        assert_eq!(out, serde_json::to_string_pretty(&items).unwrap());
    }

    #[test]
    fn table_shows_columns_and_values() {
        let items =
            vec![json!({"id": "f1", "name": "Welcome", "published": true, "extra": "hidden"})];
        let out = render_list(&items, Format::Table, &["id", "name", "published"]);
        assert!(out.contains("f1") && out.contains("Welcome") && out.contains("true"));
        assert!(out.contains("id") && out.contains("published")); // headers
        assert!(!out.contains("hidden")); // only requested columns
    }

    #[test]
    fn missing_column_renders_empty_not_panic() {
        let items = vec![json!({"id": "f1"})];
        let out = render_list(&items, Format::Table, &["id", "name"]);
        assert!(out.contains("f1"));
    }

    #[test]
    fn item_renders_key_value_lines() {
        let v = json!({"id": "f1", "name": "Welcome", "tags": {"t1": true}});
        let out = render_item(&v, Format::Table);

        // Verify all content is present
        assert!(out.contains("id"));
        assert!(out.contains("f1"));
        assert!(out.contains("name"));
        assert!(out.contains("Welcome"));
        assert!(out.contains("tags"));
        assert!(out.contains("{\"t1\":true}")); // nested → compact json

        // Verify alignment: check that all ": " are at the same column
        let lines: Vec<&str> = out.lines().collect();
        assert!(lines.len() >= 3, "Should have at least 3 lines for 3 keys");

        let mut colon_positions = Vec::new();
        for line in &lines {
            if let Some(pos) = line.find(':') {
                colon_positions.push(pos);
            }
        }

        // All colons should be at the same position (keys padded to longest width)
        assert!(!colon_positions.is_empty(), "Should have colons in output");
        let first = colon_positions[0];
        for &pos in &colon_positions[1..] {
            assert_eq!(pos, first, "All key:value pairs should be aligned");
        }
    }

    #[test]
    fn item_renders_scalar_value() {
        let scalar_str = json!("just a string");
        let out = render_item(&scalar_str, Format::Table);
        assert_eq!(out, "just a string");

        let scalar_num = json!(42);
        let out = render_item(&scalar_num, Format::Table);
        assert_eq!(out, "42");
    }
}
