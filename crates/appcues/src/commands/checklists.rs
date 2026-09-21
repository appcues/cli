use super::{Ctx, as_list};
use crate::output::{render_item, render_list};
use anyhow::Result;

pub fn list(ctx: &Ctx) -> Result<String> {
    let v = ctx.client.get(&ctx.path("checklists"))?;
    Ok(render_list(
        &as_list(v),
        ctx.format,
        &["id", "name", "published", "updated_at"],
    ))
}

pub fn get(ctx: &Ctx, checklist_id: &str) -> Result<String> {
    let v = ctx
        .client
        .get(&ctx.path(&format!("checklists/{checklist_id}")))?;
    Ok(render_item(&v, ctx.format))
}

pub fn publish(ctx: &Ctx, checklist_id: &str) -> Result<String> {
    let path = ctx.path(&format!("checklists/{checklist_id}/publish"));
    if let Some(msg) = ctx.dry_run_output("POST", &path, None) {
        return Ok(msg);
    }
    ctx.client.post(&path, None)?;
    Ok(format!("Published checklist {checklist_id}"))
}

pub fn unpublish(ctx: &Ctx, checklist_id: &str) -> Result<String> {
    let path = ctx.path(&format!("checklists/{checklist_id}/unpublish"));
    if let Some(msg) = ctx.dry_run_output("POST", &path, None) {
        return Ok(msg);
    }
    ctx.client.post(&path, None)?;
    Ok(format!("Unpublished checklist {checklist_id}"))
}
