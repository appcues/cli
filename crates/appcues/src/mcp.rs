//! MCP (Model Context Protocol) server over stdio: the same `commands::*`
//! functions the CLI dispatches to, exposed as typed tools for agents.
//!
//! Every tool runs the shared command function with the process's `Ctx`
//! (profile credentials, account, base URLs) and returns its rendered
//! output as text: JSON for reads, a confirmation line for writes. A
//! failure becomes an `isError` result whose text is the CLI's one-line
//! JSON error (`type`, `status`, `message`, `body`, ...), so an agent can
//! branch on it exactly like a script branches on stderr.
//!
//! stdout is the protocol channel; nothing here prints to it. The command
//! layer's informational lines (`emit_meta`) already go to stderr.

use crate::commands::experiences::ExperienceType;
use crate::commands::{self, Ctx};
use crate::output::{Format, render_error};
use anyhow::{Context, Result, bail};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock, Implementation, ServerCapabilities, ServerConfig};
use rmcp::{ErrorData, ServerHandler, ServiceExt, tool, tool_handler, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Map, Value};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

const INSTRUCTIONS: &str = "Tools for one Appcues account, authenticated with the credentials \
saved in the local appcues CLI profile (the account id is fixed by that profile). Reads return \
JSON exactly as the Appcues API sends it; writes return a short confirmation. A failed call \
returns an isError result whose text is a JSON object with `type` (config, auth, api, \
rate_limited, server, tool, unexpected), `status` (HTTP status when any), `message`, and the \
API error `body` when the server sent one: read it to decide whether to fix the arguments, \
back off, or stop. Analytics specs are validated by the server; its 400 body names the \
offending field. Long exports: start_analytics_export, then get_export_job or \
download_export_job with the returned job_id.";

/// Default wait for the job-polling tools. Shorter than the CLI's 15 minutes
/// because MCP clients time out tool calls; the agent polls again instead.
const DEFAULT_JOB_TIMEOUT_SECS: u64 = 120;

#[derive(Clone)]
pub struct AppcuesMcp {
    ctx: Arc<Ctx>,
}

impl AppcuesMcp {
    /// Wrap a CLI context for MCP use: output is always JSON (the agent
    /// parses it), and interactive prompting is forced off because stdin
    /// is the protocol transport, not a terminal. `dry_run` is kept, so
    /// `appcues --dry-run mcp` previews every write without sending it.
    pub fn new(mut ctx: Ctx) -> Self {
        ctx.format = Format::Json;
        ctx.interactive = false;
        AppcuesMcp { ctx: Arc::new(ctx) }
    }

    /// Run a blocking command function off the async runtime and map its
    /// result: Ok(text) → success content, Err → isError with the CLI's
    /// one-line JSON error as the text.
    pub async fn run(
        &self,
        f: impl FnOnce(&Ctx) -> Result<String> + Send + 'static,
    ) -> Result<CallToolResult, ErrorData> {
        let ctx = Arc::clone(&self.ctx);
        let out = tokio::task::spawn_blocking(move || f(&ctx))
            .await
            .map_err(|e| ErrorData::internal_error(format!("tool task failed: {e}"), None))?;
        Ok(match out {
            Ok(text) => CallToolResult::success(vec![ContentBlock::text(text)]),
            Err(e) => CallToolResult::error(vec![ContentBlock::text(render_error(&e).1)]),
        })
    }
}

fn days_in_range(days: Option<u32>) -> Result<u32> {
    let days = days.unwrap_or(7);
    if !(1..=90).contains(&days) {
        bail!("days must be between 1 and 90, got {days}");
    }
    Ok(days)
}

/// Model-supplied output paths stay inside the server's working directory,
/// the contract the tool descriptions state: relative, no `..`, and not
/// routed through a symlink that leaves the directory. `base` is the
/// working directory (a parameter so tests can use a temp dir).
fn local_out_path(base: &Path, p: Option<&str>) -> Result<Option<PathBuf>> {
    let Some(p) = p else { return Ok(None) };
    let path = Path::new(p);
    let escapes = path
        .components()
        .any(|c| !matches!(c, Component::Normal(_) | Component::CurDir));
    if path.is_absolute() || escapes {
        bail!("out_path must be a relative path inside the working directory, without `..`: {p}");
    }
    // Components are plain names, so the only way out is a symlinked
    // directory on the way: resolve the parent and check it stays inside.
    let base = base
        .canonicalize()
        .context("cannot resolve the working directory")?;
    let parent = path.parent().filter(|d| !d.as_os_str().is_empty());
    let real_parent = base
        .join(parent.unwrap_or(Path::new(".")))
        .canonicalize()
        .with_context(|| format!("out_path directory does not exist: {p}"))?;
    if !real_parent.starts_with(&base) {
        bail!("out_path leaves the working directory through a symlink: {p}");
    }
    // The file itself and the .part sibling save_download writes first
    // must not be symlinks either (create would follow them).
    for candidate in [path.to_path_buf(), PathBuf::from(format!("{p}.part"))] {
        let is_link = std::fs::symlink_metadata(base.join(&candidate))
            .is_ok_and(|m| m.file_type().is_symlink());
        if is_link {
            bail!("out_path must not be a symlink: {}", candidate.display());
        }
    }
    Ok(Some(path.to_path_buf()))
}

fn timeout(secs: Option<u64>) -> Duration {
    Duration::from_secs(secs.unwrap_or(DEFAULT_JOB_TIMEOUT_SECS))
}

// ---- parameter types ------------------------------------------------------

#[derive(Deserialize, JsonSchema)]
pub struct FlowId {
    /// The flow's id (from list_flows)
    pub flow_id: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct DigestParams {
    /// Window length in days ending now, 1-90 (default 7); the previous
    /// period of the same length is derived for the deltas
    pub days: Option<u32>,
}

#[derive(Deserialize, JsonSchema)]
pub struct ExperienceTypeParam {
    /// Which experience resource to use; each type is its own API route
    pub experience_type: ExperienceType,
}

#[derive(Deserialize, JsonSchema)]
pub struct ExperienceRef {
    /// Which experience resource to use; each type is its own API route
    pub experience_type: ExperienceType,
    /// The experience's id (from list_experiences)
    pub experience_id: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct ChecklistId {
    /// The checklist's id (from list_checklists)
    pub checklist_id: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct TagId {
    /// The tag's id (from list_tags)
    pub tag_id: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct ScreenshotParams {
    /// Id of a flow, experience, or checklist; only draft content has
    /// screenshots
    pub resource_id: String,
    /// Where to write the ZIP, relative to the server's working directory
    /// (default: <resource_id>-screenshots.zip)
    pub out_path: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct SegmentId {
    /// The segment's id (from list_segments)
    pub segment_id: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct CreateSegmentParams {
    /// Display name of the new segment
    pub name: String,
    /// Optional description
    pub description: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct UpdateSegmentParams {
    /// The segment's id (from list_segments)
    pub segment_id: String,
    /// New name; omit to keep the current one
    pub name: Option<String>,
    /// New description; omit to keep the current one
    pub description: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct SegmentUsers {
    /// The segment's id (from list_segments)
    pub segment_id: String,
    /// End-user ids (the ids your application identifies users with)
    pub user_ids: Vec<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct GroupId {
    /// The group's id (the id your application identifies groups with)
    pub group_id: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct UpdateGroupParams {
    /// The group's id
    pub group_id: String,
    /// Attributes to set, as a JSON object; values keep their JSON types
    pub attributes: Map<String, Value>,
}

#[derive(Deserialize, JsonSchema)]
pub struct GroupUsers {
    /// The group's id
    pub group_id: String,
    /// End-user ids to associate with the group
    pub user_ids: Vec<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct UserId {
    /// The end-user's id (the id your application identifies users with)
    pub user_id: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct UpdateUserParams {
    /// The end-user's id
    pub user_id: String,
    /// Profile attributes to set, as a JSON object; values keep their JSON
    /// types
    pub attributes: Map<String, Value>,
}

#[derive(Deserialize, JsonSchema)]
pub struct UserEventsParams {
    /// The end-user's id
    pub user_id: String,
    /// Maximum number of events to return (server default when omitted)
    pub limit: Option<u32>,
}

#[derive(Deserialize, JsonSchema)]
pub struct TrackEventParams {
    /// The end-user's id
    pub user_id: String,
    /// Event name
    pub name: String,
    /// RFC 3339 timestamp, e.g. 2026-08-12T00:00:00Z (default: now)
    pub timestamp: Option<String>,
    /// Event attributes as a JSON object (default: none)
    pub attributes: Option<Map<String, Value>>,
}

#[derive(Deserialize, JsonSchema)]
pub struct AnalyticsSpec {
    /// The analytics spec object: `metrics` + `dimensions` for aggregates,
    /// or `columns` for raw event rows, plus `conditions`, `start_time`,
    /// `end_time`
    pub spec: Value,
}

#[derive(Deserialize, JsonSchema)]
pub struct CompareParams {
    /// An aggregate spec (`metrics` + `dimensions`); its start_time and
    /// end_time are replaced by the derived windows
    pub spec: Value,
    /// Window length in days ending now, 1-90 (default 7)
    pub days: Option<u32>,
}

#[derive(Deserialize, JsonSchema)]
pub struct JobId {
    /// The export job id returned by start_analytics_export
    pub job_id: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct WaitJobParams {
    /// The export job id returned by start_analytics_export
    pub job_id: String,
    /// Give up after this many seconds (default 120); call again to keep
    /// waiting
    pub timeout_secs: Option<u64>,
}

#[derive(Deserialize, JsonSchema)]
pub struct DownloadJobParams {
    /// The export job id returned by start_analytics_export
    pub job_id: String,
    /// Where to write the result, relative to the server's working
    /// directory (default: <job_id>.json)
    pub out_path: Option<String>,
    /// Give up after this many seconds (default 120)
    pub timeout_secs: Option<u64>,
}

#[derive(Deserialize, JsonSchema)]
pub struct ListAccountToolsParams {
    /// Include each tool's inputSchema and annotations (default: name,
    /// role, description only)
    pub full: Option<bool>,
}

#[derive(Deserialize, JsonSchema)]
pub struct AccountToolName {
    /// The account tool's name (from list_account_tools)
    pub name: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct CallAccountToolParams {
    /// The account tool's name (from list_account_tools)
    pub name: String,
    /// Arguments matching the tool's inputSchema (default: none)
    pub arguments: Option<Map<String, Value>>,
}

// ---- tools ----------------------------------------------------------------

#[tool_router(vis = "pub")]
impl AppcuesMcp {
    /// Check that the saved credentials work against the Appcues API and
    /// report which account and API origin this server is bound to.
    #[tool(annotations(title = "Verify credentials", read_only_hint = true))]
    pub async fn verify_credentials(&self) -> Result<CallToolResult, ErrorData> {
        self.run(commands::profiles::status).await
    }

    /// List all flows in the account (id, name, published, timestamps).
    #[tool(annotations(title = "List flows", read_only_hint = true))]
    pub async fn list_flows(&self) -> Result<CallToolResult, ErrorData> {
        self.run(commands::flows::list).await
    }

    /// Show one flow's full definition.
    #[tool(annotations(title = "Get flow", read_only_hint = true))]
    pub async fn get_flow(
        &self,
        Parameters(p): Parameters<FlowId>,
    ) -> Result<CallToolResult, ErrorData> {
        self.run(move |ctx| commands::flows::get(ctx, &p.flow_id))
            .await
    }

    /// Publish a flow so it is live for end users.
    #[tool(annotations(title = "Publish flow", idempotent_hint = true))]
    pub async fn publish_flow(
        &self,
        Parameters(p): Parameters<FlowId>,
    ) -> Result<CallToolResult, ErrorData> {
        self.run(move |ctx| commands::flows::publish(ctx, &p.flow_id))
            .await
    }

    /// Unpublish a flow so end users stop seeing it.
    #[tool(annotations(title = "Unpublish flow", idempotent_hint = true))]
    pub async fn unpublish_flow(
        &self,
        Parameters(p): Parameters<FlowId>,
    ) -> Result<CallToolResult, ErrorData> {
        self.run(move |ctx| commands::flows::unpublish(ctx, &p.flow_id))
            .await
    }

    /// Performance digest of every published flow over the last N days
    /// versus the same-length period before: shown, completed, skipped,
    /// errors, unique users, completion rate, and deltas.
    #[tool(annotations(title = "Flow performance digest", read_only_hint = true))]
    pub async fn flow_performance_digest(
        &self,
        Parameters(p): Parameters<DigestParams>,
    ) -> Result<CallToolResult, ErrorData> {
        self.run(move |ctx| commands::flows::digest(ctx, days_in_range(p.days)?, SystemTime::now()))
            .await
    }

    /// List all experiences of one type: pins, mobile, launchpads,
    /// banners, flows-v2, embeds, or nps.
    #[tool(annotations(title = "List experiences", read_only_hint = true))]
    pub async fn list_experiences(
        &self,
        Parameters(p): Parameters<ExperienceTypeParam>,
    ) -> Result<CallToolResult, ErrorData> {
        self.run(move |ctx| commands::experiences::list(ctx, p.experience_type))
            .await
    }

    /// Show one experience's full definition.
    #[tool(annotations(title = "Get experience", read_only_hint = true))]
    pub async fn get_experience(
        &self,
        Parameters(p): Parameters<ExperienceRef>,
    ) -> Result<CallToolResult, ErrorData> {
        self.run(move |ctx| commands::experiences::get(ctx, p.experience_type, &p.experience_id))
            .await
    }

    /// Publish an experience so it is live for end users.
    #[tool(annotations(title = "Publish experience", idempotent_hint = true))]
    pub async fn publish_experience(
        &self,
        Parameters(p): Parameters<ExperienceRef>,
    ) -> Result<CallToolResult, ErrorData> {
        self.run(move |ctx| {
            commands::experiences::publish(ctx, p.experience_type, &p.experience_id)
        })
        .await
    }

    /// Unpublish an experience so end users stop seeing it.
    #[tool(annotations(title = "Unpublish experience", idempotent_hint = true))]
    pub async fn unpublish_experience(
        &self,
        Parameters(p): Parameters<ExperienceRef>,
    ) -> Result<CallToolResult, ErrorData> {
        self.run(move |ctx| {
            commands::experiences::unpublish(ctx, p.experience_type, &p.experience_id)
        })
        .await
    }

    /// List all checklists in the account.
    #[tool(annotations(title = "List checklists", read_only_hint = true))]
    pub async fn list_checklists(&self) -> Result<CallToolResult, ErrorData> {
        self.run(commands::checklists::list).await
    }

    /// Show one checklist's full definition.
    #[tool(annotations(title = "Get checklist", read_only_hint = true))]
    pub async fn get_checklist(
        &self,
        Parameters(p): Parameters<ChecklistId>,
    ) -> Result<CallToolResult, ErrorData> {
        self.run(move |ctx| commands::checklists::get(ctx, &p.checklist_id))
            .await
    }

    /// Publish a checklist so it is live for end users.
    #[tool(annotations(title = "Publish checklist", idempotent_hint = true))]
    pub async fn publish_checklist(
        &self,
        Parameters(p): Parameters<ChecklistId>,
    ) -> Result<CallToolResult, ErrorData> {
        self.run(move |ctx| commands::checklists::publish(ctx, &p.checklist_id))
            .await
    }

    /// Unpublish a checklist so end users stop seeing it.
    #[tool(annotations(title = "Unpublish checklist", idempotent_hint = true))]
    pub async fn unpublish_checklist(
        &self,
        Parameters(p): Parameters<ChecklistId>,
    ) -> Result<CallToolResult, ErrorData> {
        self.run(move |ctx| commands::checklists::unpublish(ctx, &p.checklist_id))
            .await
    }

    /// List all tags in the account.
    #[tool(annotations(title = "List tags", read_only_hint = true))]
    pub async fn list_tags(&self) -> Result<CallToolResult, ErrorData> {
        self.run(commands::tags::list).await
    }

    /// Show one tag.
    #[tool(annotations(title = "Get tag", read_only_hint = true))]
    pub async fn get_tag(
        &self,
        Parameters(p): Parameters<TagId>,
    ) -> Result<CallToolResult, ErrorData> {
        self.run(move |ctx| commands::tags::get(ctx, &p.tag_id))
            .await
    }

    /// Download a flow, experience, or checklist's draft screenshots as a
    /// ZIP file on the local disk; returns the path and byte count.
    #[tool(annotations(
        title = "Download screenshots",
        read_only_hint = false,
        idempotent_hint = true
    ))]
    pub async fn download_screenshots(
        &self,
        Parameters(p): Parameters<ScreenshotParams>,
    ) -> Result<CallToolResult, ErrorData> {
        self.run(move |ctx| {
            commands::screenshots::download(
                ctx,
                &p.resource_id,
                local_out_path(&std::env::current_dir()?, p.out_path.as_deref())?.as_deref(),
            )
        })
        .await
    }

    /// List all segments in the account.
    #[tool(annotations(title = "List segments", read_only_hint = true))]
    pub async fn list_segments(&self) -> Result<CallToolResult, ErrorData> {
        self.run(commands::segments::list).await
    }

    /// Show one segment.
    #[tool(annotations(title = "Get segment", read_only_hint = true))]
    pub async fn get_segment(
        &self,
        Parameters(p): Parameters<SegmentId>,
    ) -> Result<CallToolResult, ErrorData> {
        self.run(move |ctx| commands::segments::get(ctx, &p.segment_id))
            .await
    }

    /// Create a segment; returns the new segment including its id.
    #[tool(annotations(title = "Create segment", destructive_hint = false))]
    pub async fn create_segment(
        &self,
        Parameters(p): Parameters<CreateSegmentParams>,
    ) -> Result<CallToolResult, ErrorData> {
        self.run(move |ctx| commands::segments::create(ctx, &p.name, p.description.as_deref()))
            .await
    }

    /// Rename a segment or change its description.
    #[tool(annotations(title = "Update segment", idempotent_hint = true))]
    pub async fn update_segment(
        &self,
        Parameters(p): Parameters<UpdateSegmentParams>,
    ) -> Result<CallToolResult, ErrorData> {
        self.run(move |ctx| {
            commands::segments::update(
                ctx,
                &p.segment_id,
                p.name.as_deref(),
                p.description.as_deref(),
            )
        })
        .await
    }

    /// Permanently delete a segment. This cannot be undone.
    #[tool(annotations(title = "Delete segment", destructive_hint = true))]
    pub async fn delete_segment(
        &self,
        Parameters(p): Parameters<SegmentId>,
    ) -> Result<CallToolResult, ErrorData> {
        self.run(move |ctx| commands::segments::delete(ctx, &p.segment_id))
            .await
    }

    /// Add end users to a manual segment by their user ids.
    #[tool(annotations(
        title = "Add users to segment",
        destructive_hint = false,
        idempotent_hint = true
    ))]
    pub async fn add_users_to_segment(
        &self,
        Parameters(p): Parameters<SegmentUsers>,
    ) -> Result<CallToolResult, ErrorData> {
        self.run(move |ctx| commands::segments::add_users(ctx, &p.segment_id, &p.user_ids))
            .await
    }

    /// Remove end users from a manual segment by their user ids.
    #[tool(annotations(title = "Remove users from segment", idempotent_hint = true))]
    pub async fn remove_users_from_segment(
        &self,
        Parameters(p): Parameters<SegmentUsers>,
    ) -> Result<CallToolResult, ErrorData> {
        self.run(move |ctx| commands::segments::remove_users(ctx, &p.segment_id, &p.user_ids))
            .await
    }

    /// Show a group's profile attributes.
    #[tool(annotations(title = "Get group", read_only_hint = true))]
    pub async fn get_group(
        &self,
        Parameters(p): Parameters<GroupId>,
    ) -> Result<CallToolResult, ErrorData> {
        self.run(move |ctx| commands::groups::get(ctx, &p.group_id))
            .await
    }

    /// Set attributes on a group's profile (existing attributes not named
    /// are kept).
    #[tool(annotations(title = "Update group", idempotent_hint = true))]
    pub async fn update_group(
        &self,
        Parameters(p): Parameters<UpdateGroupParams>,
    ) -> Result<CallToolResult, ErrorData> {
        self.run(move |ctx| commands::groups::update(ctx, &p.group_id, p.attributes))
            .await
    }

    /// Associate end users with a group by their user ids.
    #[tool(annotations(
        title = "Add users to group",
        destructive_hint = false,
        idempotent_hint = true
    ))]
    pub async fn add_users_to_group(
        &self,
        Parameters(p): Parameters<GroupUsers>,
    ) -> Result<CallToolResult, ErrorData> {
        self.run(move |ctx| commands::groups::add_users(ctx, &p.group_id, &p.user_ids))
            .await
    }

    /// Show an end user's profile attributes.
    #[tool(annotations(title = "Get user", read_only_hint = true))]
    pub async fn get_user(
        &self,
        Parameters(p): Parameters<UserId>,
    ) -> Result<CallToolResult, ErrorData> {
        self.run(move |ctx| commands::users::get(ctx, &p.user_id))
            .await
    }

    /// Set attributes on an end user's profile (existing attributes not
    /// named are kept).
    #[tool(annotations(title = "Update user", idempotent_hint = true))]
    pub async fn update_user(
        &self,
        Parameters(p): Parameters<UpdateUserParams>,
    ) -> Result<CallToolResult, ErrorData> {
        self.run(move |ctx| commands::users::update(ctx, &p.user_id, p.attributes))
            .await
    }

    /// Permanently delete an end user's profile. This cannot be undone.
    #[tool(annotations(title = "Delete user", destructive_hint = true))]
    pub async fn delete_user(
        &self,
        Parameters(p): Parameters<UserId>,
    ) -> Result<CallToolResult, ErrorData> {
        self.run(move |ctx| commands::users::delete(ctx, &p.user_id))
            .await
    }

    /// List an end user's recent events (name and timestamp).
    #[tool(annotations(title = "Get user events", read_only_hint = true))]
    pub async fn get_user_events(
        &self,
        Parameters(p): Parameters<UserEventsParams>,
    ) -> Result<CallToolResult, ErrorData> {
        self.run(move |ctx| commands::users::events(ctx, &p.user_id, p.limit))
            .await
    }

    /// Record an event for an end user, optionally with attributes and a
    /// past timestamp.
    #[tool(annotations(title = "Track user event", destructive_hint = false))]
    pub async fn track_user_event(
        &self,
        Parameters(p): Parameters<TrackEventParams>,
    ) -> Result<CallToolResult, ErrorData> {
        self.run(move |ctx| {
            commands::users::track(
                ctx,
                &p.user_id,
                &p.name,
                p.timestamp.as_deref(),
                p.attributes.unwrap_or_default(),
            )
        })
        .await
    }

    /// Run an analytics spec synchronously and return its rows: an
    /// aggregate (`metrics` + `dimensions`) or raw events (`columns`). The
    /// server may truncate large results; use start_analytics_export for
    /// full dumps.
    #[tool(annotations(title = "Run analytics query", read_only_hint = true))]
    pub async fn run_analytics_query(
        &self,
        Parameters(p): Parameters<AnalyticsSpec>,
    ) -> Result<CallToolResult, ErrorData> {
        self.run(move |ctx| commands::analytics::query(ctx, &p.spec, false))
            .await
    }

    /// Submit an analytics spec as an asynchronous export job; returns a
    /// job_id to pass to get_export_job, wait_for_export_job, or
    /// download_export_job.
    #[tool(annotations(title = "Start analytics export", destructive_hint = false))]
    pub async fn start_analytics_export(
        &self,
        Parameters(p): Parameters<AnalyticsSpec>,
    ) -> Result<CallToolResult, ErrorData> {
        self.run(move |ctx| commands::analytics::query(ctx, &p.spec, true))
            .await
    }

    /// Run an aggregate spec over the last N days and the same-length
    /// period before, joined per dimension value with current, previous,
    /// and percent deltas per metric.
    #[tool(annotations(title = "Compare analytics periods", read_only_hint = true))]
    pub async fn compare_analytics_periods(
        &self,
        Parameters(p): Parameters<CompareParams>,
    ) -> Result<CallToolResult, ErrorData> {
        self.run(move |ctx| {
            commands::analytics::compare(ctx, &p.spec, days_in_range(p.days)?, SystemTime::now())
        })
        .await
    }

    /// Show an analytics export job's status (queued, running, done, or
    /// failed).
    #[tool(annotations(title = "Get export job", read_only_hint = true))]
    pub async fn get_export_job(
        &self,
        Parameters(p): Parameters<JobId>,
    ) -> Result<CallToolResult, ErrorData> {
        self.run(move |ctx| commands::jobs::get(ctx, &p.job_id))
            .await
    }

    /// Block until an analytics export job is done or failed, or the
    /// timeout passes; returns the final job status.
    #[tool(annotations(title = "Wait for export job", read_only_hint = true))]
    pub async fn wait_for_export_job(
        &self,
        Parameters(p): Parameters<WaitJobParams>,
    ) -> Result<CallToolResult, ErrorData> {
        self.run(move |ctx| commands::jobs::wait(ctx, &p.job_id, timeout(p.timeout_secs)))
            .await
    }

    /// Wait for an analytics export job, then download its result to a
    /// local file; returns the path and byte count.
    #[tool(annotations(
        title = "Download export job",
        read_only_hint = false,
        idempotent_hint = true
    ))]
    pub async fn download_export_job(
        &self,
        Parameters(p): Parameters<DownloadJobParams>,
    ) -> Result<CallToolResult, ErrorData> {
        self.run(move |ctx| {
            commands::jobs::download(
                ctx,
                &p.job_id,
                local_out_path(&std::env::current_dir()?, p.out_path.as_deref())?.as_deref(),
                timeout(p.timeout_secs),
            )
        })
        .await
    }

    /// List the account tools this API key can call (name, role,
    /// description); use describe_account_tool for one tool's input
    /// schema and call_account_tool to run it.
    #[tool(annotations(title = "List account tools", read_only_hint = true))]
    pub async fn list_account_tools(
        &self,
        Parameters(p): Parameters<ListAccountToolsParams>,
    ) -> Result<CallToolResult, ErrorData> {
        self.run(move |ctx| commands::tools::list(ctx, p.full.unwrap_or(false)))
            .await
    }

    /// Show one account tool's entry, including its inputSchema and
    /// annotations.
    #[tool(annotations(title = "Describe account tool", read_only_hint = true))]
    pub async fn describe_account_tool(
        &self,
        Parameters(p): Parameters<AccountToolName>,
    ) -> Result<CallToolResult, ErrorData> {
        self.run(move |ctx| commands::tools::describe(ctx, &p.name))
            .await
    }

    /// Call an account tool by name with arguments matching its
    /// inputSchema. Its content (text, images) is returned as-is.
    #[tool(annotations(title = "Call account tool", open_world_hint = true))]
    pub async fn call_account_tool(
        &self,
        Parameters(p): Parameters<CallAccountToolParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let ctx = Arc::clone(&self.ctx);
        let arguments = Value::Object(p.arguments.unwrap_or_default());
        let out =
            tokio::task::spawn_blocking(move || commands::tools::invoke(&ctx, &p.name, arguments))
                .await
                .map_err(|e| ErrorData::internal_error(format!("tool task failed: {e}"), None))?;
        Ok(match out {
            Ok(commands::tools::Invoked::DryRun(msg)) => {
                CallToolResult::success(vec![ContentBlock::text(msg)])
            }
            Ok(commands::tools::Invoked::Envelope(envelope)) => envelope_result(&envelope),
            Err(e) => CallToolResult::error(vec![ContentBlock::text(render_error(&e).1)]),
        })
    }
}

/// Forward an account tool's result envelope as MCP content: text and
/// image items map one to one, `isError` becomes the result's error flag.
fn envelope_result(envelope: &Value) -> CallToolResult {
    let content: Vec<ContentBlock> = envelope
        .get("content")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| match item.get("type").and_then(Value::as_str) {
                    Some("text") => Some(ContentBlock::text(
                        item.get("text").and_then(Value::as_str).unwrap_or(""),
                    )),
                    Some("image") => Some(ContentBlock::image(
                        item.get("data").and_then(Value::as_str).unwrap_or(""),
                        item.get("mimeType")
                            .and_then(Value::as_str)
                            .unwrap_or("image/png"),
                    )),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default();
    if envelope
        .get("isError")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        CallToolResult::error(content)
    } else {
        CallToolResult::success(content)
    }
}

#[tool_handler]
impl ServerHandler for AppcuesMcp {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("appcues", env!("CARGO_PKG_VERSION")))
            .with_instructions(INSTRUCTIONS)
    }
}

/// Serve MCP over stdin/stdout until the client disconnects.
pub async fn serve(ctx: Ctx) -> Result<()> {
    let running = AppcuesMcp::new(ctx).serve(rmcp::transport::stdio()).await?;
    running.waiting().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::local_out_path;

    #[test]
    fn plain_relative_paths_pass_and_none_is_none() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        assert!(local_out_path(dir.path(), None).unwrap().is_none());
        assert!(local_out_path(dir.path(), Some("a.zip")).is_ok());
        assert!(local_out_path(dir.path(), Some("./a.zip")).is_ok());
        assert!(local_out_path(dir.path(), Some("sub/a.zip")).is_ok());
    }

    #[test]
    fn absolute_and_parent_components_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        for bad in ["/tmp/a.zip", "../a.zip", "ok/../../a.zip"] {
            let err = local_out_path(dir.path(), Some(bad)).unwrap_err();
            assert!(err.to_string().contains("out_path"), "{bad}: {err}");
        }
    }

    #[test]
    fn missing_parent_directory_is_reported_before_any_request() {
        let dir = tempfile::tempdir().unwrap();
        let err = local_out_path(dir.path(), Some("nope/a.zip")).unwrap_err();
        assert!(err.to_string().contains("does not exist"), "{err}");
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_leaving_the_directory_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), dir.path().join("escape")).unwrap();
        std::os::unix::fs::symlink(outside.path().join("f"), dir.path().join("link.zip")).unwrap();
        std::os::unix::fs::symlink(outside.path().join("p"), dir.path().join("x.zip.part"))
            .unwrap();
        // A symlink that stays inside is fine.
        std::fs::create_dir(dir.path().join("inner")).unwrap();
        std::os::unix::fs::symlink(dir.path().join("inner"), dir.path().join("alias")).unwrap();

        let err = local_out_path(dir.path(), Some("escape/a.zip")).unwrap_err();
        assert!(err.to_string().contains("symlink"), "{err}");
        let err = local_out_path(dir.path(), Some("link.zip")).unwrap_err();
        assert!(err.to_string().contains("symlink"), "{err}");
        let err = local_out_path(dir.path(), Some("x.zip")).unwrap_err();
        assert!(err.to_string().contains("symlink"), "{err}");
        assert!(local_out_path(dir.path(), Some("alias/a.zip")).is_ok());
    }
}
