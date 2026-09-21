use super::{Ctx, as_list, confirm};
use crate::output::{render_item, render_list};
use anyhow::Result;
use serde_json::{Value, json};

pub fn list(ctx: &Ctx) -> Result<String> {
    let v = ctx.client.get(&ctx.path("segments"))?;
    Ok(render_list(
        &as_list(v),
        ctx.format,
        &["id", "name", "description", "updated_at"],
    ))
}

pub fn get(ctx: &Ctx, id: &str) -> Result<String> {
    let v = ctx.client.get(&ctx.path(&format!("segments/{id}")))?;
    Ok(render_item(&v, ctx.format))
}

fn name_desc_body(name: Option<&str>, description: Option<&str>) -> Value {
    let mut map = serde_json::Map::new();
    if let Some(n) = name {
        map.insert("name".into(), json!(n));
    }
    if let Some(d) = description {
        map.insert("description".into(), json!(d));
    }
    Value::Object(map)
}

pub fn create(ctx: &Ctx, name: &str, description: Option<&str>) -> Result<String> {
    let body = name_desc_body(Some(name), description);
    let path = ctx.path("segments");
    if let Some(msg) = ctx.dry_run_output("POST", &path, Some(&body)) {
        return Ok(msg);
    }
    let v = ctx.client.post(&path, Some(&body))?;
    Ok(render_item(&v, ctx.format))
}

pub fn update(
    ctx: &Ctx,
    id: &str,
    name: Option<&str>,
    description: Option<&str>,
) -> Result<String> {
    let body = name_desc_body(name, description);
    let path = ctx.path(&format!("segments/{id}"));
    if let Some(msg) = ctx.dry_run_output("PATCH", &path, Some(&body)) {
        return Ok(msg);
    }
    let v = ctx.client.patch(&path, &body)?;
    Ok(render_item(&v, ctx.format))
}

pub fn delete(ctx: &Ctx, id: &str) -> Result<String> {
    let path = ctx.path(&format!("segments/{id}"));
    if let Some(msg) = ctx.dry_run_output("DELETE", &path, None) {
        return Ok(msg);
    }
    confirm(
        &format!("Delete segment {id}? This cannot be undone."),
        ctx.interactive,
    )?;
    ctx.client.delete(&path)?;
    Ok(format!("Deleted segment {id}"))
}

pub fn add_users(ctx: &Ctx, id: &str, user_ids: &[String]) -> Result<String> {
    let path = ctx.path(&format!("segments/{id}/add_user_ids"));
    let body = json!({"user_ids": user_ids});
    if let Some(msg) = ctx.dry_run_output("POST", &path, Some(&body)) {
        return Ok(msg);
    }
    ctx.client.post(&path, Some(&body))?;
    Ok(format!("Added {} user(s) to segment {id}", user_ids.len()))
}

pub fn remove_users(ctx: &Ctx, id: &str, user_ids: &[String]) -> Result<String> {
    let path = ctx.path(&format!("segments/{id}/remove_user_ids"));
    let body = json!({"user_ids": user_ids});
    if let Some(msg) = ctx.dry_run_output("POST", &path, Some(&body)) {
        return Ok(msg);
    }
    ctx.client.post(&path, Some(&body))?;
    Ok(format!(
        "Removed {} user(s) from segment {id}",
        user_ids.len()
    ))
}
