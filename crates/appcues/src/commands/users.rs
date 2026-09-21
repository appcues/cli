use super::{Ctx, as_list, confirm};
use crate::output::{render_item, render_list};
use anyhow::Result;
use serde_json::{Map, Value, json};

pub fn get(ctx: &Ctx, user_id: &str) -> Result<String> {
    let v = ctx
        .client
        .get(&ctx.path(&format!("users/{user_id}/profile")))?;
    Ok(render_item(&v, ctx.format))
}

pub fn update(ctx: &Ctx, user_id: &str, attrs: Map<String, Value>) -> Result<String> {
    let body = Value::Object(attrs);
    let path = ctx.path(&format!("users/{user_id}/profile"));
    if let Some(msg) = ctx.dry_run_output("PATCH", &path, Some(&body)) {
        return Ok(msg);
    }
    let v = ctx.client.patch(&path, &body)?;
    Ok(render_item(&v, ctx.format))
}

pub fn delete(ctx: &Ctx, user_id: &str) -> Result<String> {
    let path = ctx.path(&format!("users/{user_id}/profile"));
    if let Some(msg) = ctx.dry_run_output("DELETE", &path, None) {
        return Ok(msg);
    }
    confirm(
        &format!("Delete profile for user {user_id}? This cannot be undone."),
        ctx.interactive,
    )?;
    ctx.client.delete(&path)?;
    Ok(format!("Deleted profile for user {user_id}"))
}

pub fn events(ctx: &Ctx, user_id: &str, limit: Option<u32>) -> Result<String> {
    let mut path = ctx.path(&format!("users/{user_id}/events"));
    if let Some(n) = limit {
        path.push_str(&format!("?limit={n}"));
    }
    let v = ctx.client.get(&path)?;
    Ok(render_list(&as_list(v), ctx.format, &["name", "timestamp"]))
}

pub fn track(
    ctx: &Ctx,
    user_id: &str,
    name: &str,
    timestamp: Option<&str>,
    attrs: Map<String, Value>,
) -> Result<String> {
    let ts = match timestamp {
        Some(t) => t.to_string(),
        None => humantime::format_rfc3339_seconds(std::time::SystemTime::now()).to_string(),
    };
    let body = json!({
        "name": name,
        "timestamp": ts,
        "attributes": Value::Object(attrs),
    });
    let path = ctx.path(&format!("users/{user_id}/events"));
    if let Some(msg) = ctx.dry_run_output("POST", &path, Some(&body)) {
        return Ok(msg);
    }
    ctx.client.post(&path, Some(&body))?;
    Ok(format!("Tracked event '{name}' for user {user_id}"))
}
