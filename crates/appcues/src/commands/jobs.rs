use super::{Ctx, emit_meta, save_download};
use crate::output::{Format, render_item};
use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

fn status_path(ctx: &Ctx, job_id: &str) -> String {
    ctx.path(&format!("analytics/exports/{job_id}"))
}

pub fn get(ctx: &Ctx, job_id: &str) -> Result<String> {
    let res = ctx.client.get_with_meta(&status_path(ctx, job_id))?;
    emit_meta(&res);
    Ok(render_item(&res.value, ctx.format))
}

/// Poll until the job reports done or failed
/// (statuses: queued | running | done | failed).
pub fn wait(ctx: &Ctx, job_id: &str, timeout: Duration) -> Result<String> {
    let res = wait_for_done(ctx, job_id, timeout)?;
    emit_meta(&res);
    Ok(render_item(&res.value, ctx.format))
}

/// Wait for the job, then stream its result to a local file, printing
/// the path. The presigned URL never appears in any output — some
/// runtimes mask credential-like strings in displayed text, which
/// corrupts a URL an agent copies back out of its own transcript.
pub fn download(ctx: &Ctx, job_id: &str, out: Option<&Path>, timeout: Duration) -> Result<String> {
    let path = out
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from(format!("{job_id}.json")));
    if let Some(msg) = ctx.dry_run_output("GET", &status_path(ctx, job_id), None) {
        return Ok(msg);
    }
    let res = wait_for_done(ctx, job_id, timeout)?;
    emit_meta(&res);
    let url = res
        .value
        .get("download_url")
        .and_then(Value::as_str)
        .context("done job has no download_url")?;
    let bytes = save_download(&path, |f| ctx.client.download(url, f))?;
    Ok(match ctx.format {
        Format::Json => render_item(
            &serde_json::json!({"path": path.display().to_string(), "bytes": bytes}),
            ctx.format,
        ),
        Format::Table => format!("Downloaded {bytes} bytes to {}", path.display()),
    })
}

/// The shared poll loop; the final response carries a freshly minted
/// presigned `download_url` (each status poll mints a new one).
fn wait_for_done(ctx: &Ctx, job_id: &str, timeout: Duration) -> Result<crate::client::Response> {
    let started = Instant::now();
    let mut poll_gap = Duration::from_secs(2);
    loop {
        let res = ctx.client.get_with_meta(&status_path(ctx, job_id))?;
        match res
            .value
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("")
        {
            "done" => return Ok(res),
            "failed" => bail!("job {job_id} failed: {}", res.value),
            _ => {}
        }
        if started.elapsed() >= timeout {
            bail!(
                "timed out waiting for job {job_id}; check later with `appcues jobs get {job_id}`"
            );
        }
        // Never sleep past the deadline: cap the gap to the time remaining.
        std::thread::sleep(poll_gap.min(timeout.saturating_sub(started.elapsed())));
        poll_gap = (poll_gap * 2).min(Duration::from_secs(30));
    }
}
