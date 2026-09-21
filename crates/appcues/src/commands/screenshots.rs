use super::{Ctx, save_download};
use crate::output::{Format, render_item};
use anyhow::Result;
use std::path::{Path, PathBuf};

/// Download a resource's draft screenshots ZIP. One API route serves every
/// experience type (flows, pins, checklists, ...): the id alone identifies
/// the resource.
pub fn download(ctx: &Ctx, resource_id: &str, out: Option<&Path>) -> Result<String> {
    let path = out
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from(format!("{resource_id}-screenshots.zip")));
    let api_path = ctx.path(&format!("screenshots/{resource_id}"));
    if let Some(msg) = ctx.dry_run_output("GET", &api_path, None) {
        return Ok(msg);
    }
    let bytes = save_download(&path, |f| ctx.client.download_api(&api_path, f))?;
    Ok(match ctx.format {
        Format::Json => render_item(
            &serde_json::json!({"path": path.display().to_string(), "bytes": bytes}),
            ctx.format,
        ),
        Format::Table => format!("Downloaded {bytes} bytes to {}", path.display()),
    })
}
