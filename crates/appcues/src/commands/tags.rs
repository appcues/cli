use super::{Ctx, as_list};
use crate::output::{render_item, render_list};
use anyhow::Result;

pub fn list(ctx: &Ctx) -> Result<String> {
    let v = ctx.client.get(&ctx.path("tags"))?;
    Ok(render_list(&as_list(v), ctx.format, &["id", "name"]))
}

pub fn get(ctx: &Ctx, tag_id: &str) -> Result<String> {
    let v = ctx.client.get(&ctx.path(&format!("tags/{tag_id}")))?;
    Ok(render_item(&v, ctx.format))
}
