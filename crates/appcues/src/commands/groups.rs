use super::Ctx;
use crate::output::render_item;
use anyhow::Result;
use serde_json::{Map, Value, json};

pub fn get(ctx: &Ctx, group_id: &str) -> Result<String> {
    let v = ctx
        .client
        .get(&ctx.path(&format!("groups/{group_id}/profile")))?;
    Ok(render_item(&v, ctx.format))
}

pub fn update(ctx: &Ctx, group_id: &str, attrs: Map<String, Value>) -> Result<String> {
    let body = Value::Object(attrs);
    let path = ctx.path(&format!("groups/{group_id}/profile"));
    if let Some(msg) = ctx.dry_run_output("PATCH", &path, Some(&body)) {
        return Ok(msg);
    }
    let v = ctx.client.patch(&path, &body)?;
    Ok(render_item(&v, ctx.format))
}

pub fn add_users(ctx: &Ctx, group_id: &str, user_ids: &[String]) -> Result<String> {
    let path = ctx.path(&format!("groups/{group_id}/users"));
    let body = json!({"user_ids": user_ids});
    if let Some(msg) = ctx.dry_run_output("POST", &path, Some(&body)) {
        return Ok(msg);
    }
    ctx.client.post(&path, Some(&body))?;
    Ok(format!(
        "Associated {} user(s) with group {group_id}",
        user_ids.len()
    ))
}
