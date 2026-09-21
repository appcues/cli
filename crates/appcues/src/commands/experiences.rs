use super::{Ctx, as_list};
use crate::output::{render_item, render_list};
use anyhow::Result;
use clap::ValueEnum;
use schemars::JsonSchema;
use serde::Deserialize;

/// The public API v2 has no single `experiences` route: each experience
/// type is its own resource. The CLI value name (kebab-case of the
/// variant) doubles as the API path segment.
#[derive(Clone, Copy, ValueEnum, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ExperienceType {
    Pins,
    Mobile,
    Launchpads,
    Banners,
    FlowsV2,
    Embeds,
    Nps,
}

impl ExperienceType {
    fn segment(&self) -> String {
        self.to_possible_value()
            .expect("no skipped variants")
            .get_name()
            .to_string()
    }
}

pub fn list(ctx: &Ctx, kind: ExperienceType) -> Result<String> {
    let v = ctx.client.get(&ctx.path(&kind.segment()))?;
    Ok(render_list(
        &as_list(v),
        ctx.format,
        &["id", "name", "published", "updated_at"],
    ))
}

pub fn get(ctx: &Ctx, kind: ExperienceType, experience_id: &str) -> Result<String> {
    let v = ctx
        .client
        .get(&ctx.path(&format!("{}/{experience_id}", kind.segment())))?;
    Ok(render_item(&v, ctx.format))
}

pub fn publish(ctx: &Ctx, kind: ExperienceType, experience_id: &str) -> Result<String> {
    let path = ctx.path(&format!("{}/{experience_id}/publish", kind.segment()));
    if let Some(msg) = ctx.dry_run_output("POST", &path, None) {
        return Ok(msg);
    }
    ctx.client.post(&path, None)?;
    Ok(format!("Published {} {experience_id}", kind.segment()))
}

pub fn unpublish(ctx: &Ctx, kind: ExperienceType, experience_id: &str) -> Result<String> {
    let path = ctx.path(&format!("{}/{experience_id}/unpublish", kind.segment()));
    if let Some(msg) = ctx.dry_run_output("POST", &path, None) {
        return Ok(msg);
    }
    ctx.client.post(&path, None)?;
    Ok(format!("Unpublished {} {experience_id}", kind.segment()))
}
