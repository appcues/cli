use anyhow::{Context, Result};
use appcues::commands::{self, Ctx};
use appcues::config;
use appcues::output::{Format, render_error};
use clap::{Parser, Subcommand};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "appcues",
    version,
    disable_version_flag = true,
    about = "CLI for the Appcues API"
)]
struct Cli {
    /// Print version
    #[arg(short = 'v', short_alias = 'V', long, action = clap::ArgAction::Version)]
    version: (),
    /// Config profile to use
    #[arg(
        long,
        global = true,
        env = "APPCUES_PROFILE",
        default_value = "default"
    )]
    profile: String,
    /// Override the profile's account ID
    #[arg(long, global = true)]
    account: Option<String>,
    /// Output format
    #[arg(short = 'o', long, global = true, value_enum, default_value_t = Format::Table)]
    output: Format,
    /// Print write requests instead of sending them (reads still run)
    #[arg(long, global = true)]
    dry_run: bool,
    /// Prompt y/N before destructive commands (default: run without
    /// asking); also set per profile with interactive = true
    #[arg(short = 'i', long, global = true)]
    interactive: bool,
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Inspect and list tags
    #[command(subcommand)]
    Tags(TagsCmd),
    /// Inspect and publish flows
    #[command(subcommand)]
    Flows(FlowsCmd),
    /// Inspect experiences (pins, mobile, launchpads, banners, flows 2.0,
    /// embeds, NPS)
    #[command(subcommand)]
    Experiences(ExperiencesCmd),
    /// Inspect checklists
    #[command(subcommand)]
    Checklists(ChecklistsCmd),
    /// Download a flow or experience's draft screenshots as a ZIP
    Screenshots {
        /// Flow, experience, or checklist id
        resource_id: String,
        /// Write the ZIP here (default: <RESOURCE_ID>-screenshots.zip)
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Manage segments
    #[command(subcommand)]
    Segments(SegmentsCmd),
    /// Manage group profiles
    #[command(subcommand)]
    Groups(GroupsCmd),
    /// Manage end-user profiles and events
    #[command(subcommand)]
    Users(UsersCmd),
    /// Run analytics queries and raw event exports from a spec
    #[command(subcommand)]
    Analytics(AnalyticsCmd),
    /// Inspect async jobs
    #[command(subcommand)]
    Jobs(JobsCmd),
    /// Discover and call account tools (the MCP tool set over HTTP)
    #[command(subcommand)]
    Tools(ToolsCmd),
    /// Manage config profiles and credentials
    #[command(subcommand)]
    Profiles(ProfilesCmd),
    /// Verify the active profile's credentials against the API
    Status,
    /// Serve the account as MCP tools over stdio for agents (Claude Code,
    /// Codex, ...); uses the active profile's credentials
    Mcp,
}

#[derive(Subcommand)]
enum ToolsCmd {
    /// List the tools available to this API key (name, role, description;
    /// use describe for one tool's schema)
    List {
        /// Fetch complete entries including inputSchema and annotations
        #[arg(long)]
        full: bool,
    },
    /// Show one tool's entry, including its input schema
    Describe { name: String },
    /// Call a tool by name; prefer this over raw API calls for anything
    /// without a typed command
    Call {
        name: String,
        /// Arguments as a JSON object, or - to read them from stdin
        #[arg(long, conflicts_with_all = ["input_file", "attrs"])]
        input: Option<String>,
        /// Read the arguments JSON object from a file
        #[arg(long, conflicts_with = "attrs")]
        input_file: Option<PathBuf>,
        /// One argument as key=value (repeatable); values parsed as JSON
        #[arg(long = "attr")]
        attrs: Vec<String>,
        /// Print the raw tool envelope instead of extracted data
        #[arg(long)]
        raw: bool,
        /// Directory for files the tool returns (default: current dir)
        #[arg(long)]
        out: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum ProfilesCmd {
    /// Save a new profile (prompts for missing values on a TTY)
    Add {
        /// Profile name
        #[arg(default_value = "default")]
        name: String,
        #[arg(long)]
        api_key: Option<String>,
        #[arg(long)]
        api_secret: Option<String>,
        #[arg(long)]
        account_id: Option<String>,
        /// prod or prod-eu
        #[arg(long)]
        env: Option<String>,
        /// Explicit API origin; wins over --env
        #[arg(long)]
        base_url: Option<String>,
        /// Explicit tools origin; wins over --env's tools URL
        #[arg(long)]
        tools_base_url: Option<String>,
    },
    /// List saved profiles (credentials truncated)
    List,
    /// Update fields of an existing profile; omitted flags keep their value
    Edit {
        name: String,
        #[arg(long)]
        api_key: Option<String>,
        #[arg(long)]
        api_secret: Option<String>,
        #[arg(long)]
        account_id: Option<String>,
        /// prod or prod-eu
        #[arg(long)]
        env: Option<String>,
        /// Explicit API origin; an empty string clears it (--env applies again)
        #[arg(long)]
        base_url: Option<String>,
        /// Explicit tools origin; an empty string clears it (--env applies again)
        #[arg(long)]
        tools_base_url: Option<String>,
    },
    /// Delete a profile from the config file
    Remove { name: String },
}

#[derive(Subcommand)]
enum TagsCmd {
    /// List all tags
    List,
    /// Show one tag
    Get { tag_id: String },
}

#[derive(Subcommand)]
enum FlowsCmd {
    /// List all flows
    List,
    /// Show one flow
    Get { flow_id: String },
    /// Publish a flow
    Publish { flow_id: String },
    /// Unpublish a flow
    Unpublish { flow_id: String },
    /// One-call digest: published flows' performance this period vs the
    /// previous period, with deltas
    #[command(name = "+digest")]
    Digest {
        /// Window length in days, ending now (previous period is derived)
        #[arg(long, default_value_t = 7, value_parser = clap::value_parser!(u32).range(1..=90))]
        days: u32,
    },
}

#[derive(Subcommand)]
enum ExperiencesCmd {
    /// List all experiences of one type
    List {
        /// Experience type; each maps to its own API route
        #[arg(value_enum)]
        r#type: commands::experiences::ExperienceType,
    },
    /// Show one experience
    Get {
        /// Experience type; each maps to its own API route
        #[arg(value_enum)]
        r#type: commands::experiences::ExperienceType,
        experience_id: String,
    },
    /// Publish an experience
    Publish {
        /// Experience type; each maps to its own API route
        #[arg(value_enum)]
        r#type: commands::experiences::ExperienceType,
        experience_id: String,
    },
    /// Unpublish an experience
    Unpublish {
        /// Experience type; each maps to its own API route
        #[arg(value_enum)]
        r#type: commands::experiences::ExperienceType,
        experience_id: String,
    },
}

#[derive(Subcommand)]
enum ChecklistsCmd {
    /// List all checklists
    List,
    /// Show one checklist
    Get { checklist_id: String },
    /// Publish a checklist
    Publish { checklist_id: String },
    /// Unpublish a checklist
    Unpublish { checklist_id: String },
}

#[derive(Subcommand)]
enum SegmentsCmd {
    /// List all segments
    List,
    /// Show one segment
    Get { segment_id: String },
    /// Create a segment
    Create {
        #[arg(long)]
        name: String,
        #[arg(long)]
        description: Option<String>,
    },
    /// Update a segment's name or description
    Update {
        segment_id: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        description: Option<String>,
    },
    /// Delete a segment
    Delete { segment_id: String },
    /// Add user IDs to a segment
    AddUsers {
        segment_id: String,
        #[arg(long, value_delimiter = ',', required = true)]
        user_ids: Vec<String>,
    },
    /// Remove user IDs from a segment
    RemoveUsers {
        segment_id: String,
        #[arg(long, value_delimiter = ',', required = true)]
        user_ids: Vec<String>,
    },
}

#[derive(Subcommand)]
enum GroupsCmd {
    /// Show a group's profile
    Get { group_id: String },
    /// Update group attributes (repeat --attr key=value)
    Update {
        group_id: String,
        #[arg(long = "attr", required = true)]
        attrs: Vec<String>,
    },
    /// Associate users with a group
    AddUsers {
        group_id: String,
        #[arg(long, value_delimiter = ',', required = true)]
        user_ids: Vec<String>,
    },
}

#[derive(Subcommand)]
enum UsersCmd {
    /// Show a user's profile
    Get { user_id: String },
    /// Update profile attributes (repeat --attr key=value)
    Update {
        user_id: String,
        #[arg(long = "attr", required = true)]
        attrs: Vec<String>,
    },
    /// Delete a user's profile
    Delete { user_id: String },
    /// Show a user's recent events
    Events {
        user_id: String,
        #[arg(long)]
        limit: Option<u32>,
    },
    /// Track an event for a user
    Track {
        user_id: String,
        #[arg(long)]
        name: String,
        /// RFC 3339, e.g. 2026-08-12T00:00:00Z (default: now)
        #[arg(long)]
        timestamp: Option<String>,
        #[arg(long = "attr")]
        attrs: Vec<String>,
    },
}

#[derive(Subcommand)]
enum AnalyticsCmd {
    /// Run a general analytics spec (sync by default)
    ///
    /// The spec's shape picks the result: `metrics` + `dimensions`
    /// returns computed aggregates, `columns` returns raw event rows.
    /// The server validates the spec and its 400s name the offending
    /// field.
    Query {
        /// Path to the spec JSON file, or - to read it from stdin
        #[arg(long)]
        spec: String,
        /// Submit as an async export job instead (returns a job_id;
        /// poll with `appcues jobs get/wait`)
        #[arg(long)]
        r#async: bool,
    },
    /// One-call comparison: run an aggregate spec over the last N days
    /// and over the same-length period before, joined per dimension
    /// value with per-metric deltas. The spec's start_time/end_time are
    /// replaced by the derived windows.
    #[command(name = "+compare")]
    Compare {
        /// Path to the spec JSON file (metrics + dimensions), or - for stdin
        #[arg(long)]
        spec: String,
        /// Window length in days, ending now (previous period is derived)
        #[arg(long, default_value_t = 7, value_parser = clap::value_parser!(u32).range(1..=90))]
        days: u32,
    },
}

#[derive(Subcommand)]
enum JobsCmd {
    /// Show an analytics export job's status
    Get { job_id: String },
    /// Poll an analytics export job until it is done or failed
    Wait {
        job_id: String,
        /// Give up after this long (e.g. 90s, 15m)
        #[arg(long, default_value = "15m", value_parser = humantime::parse_duration)]
        timeout: std::time::Duration,
    },
    /// Wait for an analytics export job, then download its result to a
    /// local file and print the path
    Download {
        job_id: String,
        /// Write the result here (default: <JOB_ID>.json)
        #[arg(long)]
        out: Option<PathBuf>,
        /// Give up after this long (e.g. 90s, 15m)
        #[arg(long, default_value = "15m", value_parser = humantime::parse_duration)]
        timeout: std::time::Duration,
    },
}

fn config_path() -> Result<PathBuf> {
    Ok(std::env::home_dir()
        .context("cannot determine home directory")?
        .join(".config/appcues/config.toml"))
}

fn build_ctx(cli: &Cli) -> Result<Ctx> {
    let env: HashMap<String, String> = std::env::vars().collect();
    let mut profile = config::load_profile(&config_path()?, &cli.profile, &env)?;
    if let Some(account) = &cli.account {
        profile.account_id = account.clone();
    }
    let tools_client = profile.tools_base_url().map(|url| {
        appcues::client::Client::with_header_auth(
            url,
            "appcues-api-key",
            appcues::client::api_key_credential(&profile.api_key, &profile.api_secret),
        )
    });
    Ok(Ctx {
        client: appcues::client::Client::new(
            profile.base_url(),
            &profile.api_key,
            &profile.api_secret,
        ),
        tools_client,
        account_id: profile.account_id,
        format: cli.output,
        dry_run: cli.dry_run,
        // The flag turns prompting on for this invocation; the profile
        // (or APPCUES_INTERACTIVE) sets the default. Off means
        // destructive commands run without asking.
        interactive: cli.interactive || profile.interactive,
    })
}

/// Profile management works on the config file alone — no credentials, so
/// no Ctx. Writes honor --dry-run; previews never include the secrets.
fn run_profiles(cli: &Cli, cmd: &ProfilesCmd) -> Result<String> {
    let config_path = config_path()?;
    let preview = |verb: &str, name: &str| match cli.output {
        Format::Json => serde_json::to_string_pretty(&serde_json::json!({
            "dry_run": true,
            "action": format!("{verb}_profile"),
            "profile": name,
            "path": config_path.display().to_string(),
        }))
        .expect("serializing Value never fails"),
        Format::Table => format!(
            "DRY RUN: would {verb} profile '{name}' in {}",
            config_path.display()
        ),
    };
    let profile_args = |api_key: &Option<String>,
                        api_secret: &Option<String>,
                        account_id: &Option<String>,
                        env: &Option<String>,
                        base_url: &Option<String>,
                        tools_base_url: &Option<String>| {
        commands::profiles::ProfileArgs {
            api_key: api_key.clone(),
            api_secret: api_secret.clone(),
            account_id: account_id.clone(),
            env: env.clone(),
            base_url: base_url.clone(),
            tools_base_url: tools_base_url.clone(),
        }
    };
    match cmd {
        ProfilesCmd::List => commands::profiles::list(&config_path, cli.output),
        ProfilesCmd::Add {
            name,
            api_key,
            api_secret,
            account_id,
            env,
            base_url,
            tools_base_url,
        } => {
            if cli.dry_run {
                return Ok(preview("write", name));
            }
            commands::profiles::add(
                &config_path,
                name,
                profile_args(
                    api_key,
                    api_secret,
                    account_id,
                    env,
                    base_url,
                    tools_base_url,
                ),
            )
        }
        ProfilesCmd::Edit {
            name,
            api_key,
            api_secret,
            account_id,
            env,
            base_url,
            tools_base_url,
        } => {
            if cli.dry_run {
                return Ok(preview("update", name));
            }
            commands::profiles::edit(
                &config_path,
                name,
                profile_args(
                    api_key,
                    api_secret,
                    account_id,
                    env,
                    base_url,
                    tools_base_url,
                ),
            )
        }
        ProfilesCmd::Remove { name } => {
            if cli.dry_run {
                return Ok(preview("remove", name));
            }
            // Same resolution as build_ctx (flag > env var > the active
            // profile's field), minus credentials: profiles commands read
            // the config file only.
            let interactive = cli.interactive
                || match std::env::var("APPCUES_INTERACTIVE") {
                    Ok(s) => config::parse_interactive(&s)?,
                    Err(_) => config::list_profiles(&config_path)?
                        .get(&cli.profile)
                        .is_some_and(|p| p.interactive),
                };
            commands::profiles::remove(&config_path, name, interactive)
        }
    }
}

fn run(cli: &Cli) -> Result<String> {
    if let Cmd::Profiles(cmd) = &cli.command {
        return run_profiles(cli, cmd);
    }
    let ctx = build_ctx(cli)?;
    match &cli.command {
        Cmd::Tags(TagsCmd::List) => commands::tags::list(&ctx),
        Cmd::Tags(TagsCmd::Get { tag_id }) => commands::tags::get(&ctx, tag_id),
        Cmd::Flows(FlowsCmd::List) => commands::flows::list(&ctx),
        Cmd::Flows(FlowsCmd::Get { flow_id }) => commands::flows::get(&ctx, flow_id),
        Cmd::Flows(FlowsCmd::Publish { flow_id }) => commands::flows::publish(&ctx, flow_id),
        Cmd::Flows(FlowsCmd::Unpublish { flow_id }) => commands::flows::unpublish(&ctx, flow_id),
        Cmd::Flows(FlowsCmd::Digest { days }) => {
            commands::flows::digest(&ctx, *days, std::time::SystemTime::now())
        }
        Cmd::Experiences(ExperiencesCmd::List { r#type }) => {
            commands::experiences::list(&ctx, *r#type)
        }
        Cmd::Experiences(ExperiencesCmd::Get {
            r#type,
            experience_id,
        }) => commands::experiences::get(&ctx, *r#type, experience_id),
        Cmd::Experiences(ExperiencesCmd::Publish {
            r#type,
            experience_id,
        }) => commands::experiences::publish(&ctx, *r#type, experience_id),
        Cmd::Experiences(ExperiencesCmd::Unpublish {
            r#type,
            experience_id,
        }) => commands::experiences::unpublish(&ctx, *r#type, experience_id),
        Cmd::Checklists(ChecklistsCmd::List) => commands::checklists::list(&ctx),
        Cmd::Checklists(ChecklistsCmd::Get { checklist_id }) => {
            commands::checklists::get(&ctx, checklist_id)
        }
        Cmd::Checklists(ChecklistsCmd::Publish { checklist_id }) => {
            commands::checklists::publish(&ctx, checklist_id)
        }
        Cmd::Checklists(ChecklistsCmd::Unpublish { checklist_id }) => {
            commands::checklists::unpublish(&ctx, checklist_id)
        }
        Cmd::Screenshots { resource_id, out } => {
            commands::screenshots::download(&ctx, resource_id, out.as_deref())
        }
        Cmd::Segments(cmd) => match cmd {
            SegmentsCmd::List => commands::segments::list(&ctx),
            SegmentsCmd::Get { segment_id } => commands::segments::get(&ctx, segment_id),
            SegmentsCmd::Create { name, description } => {
                commands::segments::create(&ctx, name, description.as_deref())
            }
            SegmentsCmd::Update {
                segment_id,
                name,
                description,
            } => commands::segments::update(
                &ctx,
                segment_id,
                name.as_deref(),
                description.as_deref(),
            ),
            SegmentsCmd::Delete { segment_id } => commands::segments::delete(&ctx, segment_id),
            SegmentsCmd::AddUsers {
                segment_id,
                user_ids,
            } => commands::segments::add_users(&ctx, segment_id, user_ids),
            SegmentsCmd::RemoveUsers {
                segment_id,
                user_ids,
            } => commands::segments::remove_users(&ctx, segment_id, user_ids),
        },
        Cmd::Groups(cmd) => match cmd {
            GroupsCmd::Get { group_id } => commands::groups::get(&ctx, group_id),
            GroupsCmd::Update { group_id, attrs } => {
                commands::groups::update(&ctx, group_id, commands::parse_attrs(attrs)?)
            }
            GroupsCmd::AddUsers { group_id, user_ids } => {
                commands::groups::add_users(&ctx, group_id, user_ids)
            }
        },
        Cmd::Users(cmd) => match cmd {
            UsersCmd::Get { user_id } => commands::users::get(&ctx, user_id),
            UsersCmd::Update { user_id, attrs } => {
                commands::users::update(&ctx, user_id, commands::parse_attrs(attrs)?)
            }
            UsersCmd::Delete { user_id } => commands::users::delete(&ctx, user_id),
            UsersCmd::Events { user_id, limit } => commands::users::events(&ctx, user_id, *limit),
            UsersCmd::Track {
                user_id,
                name,
                timestamp,
                attrs,
            } => commands::users::track(
                &ctx,
                user_id,
                name,
                timestamp.as_deref(),
                commands::parse_attrs(attrs)?,
            ),
        },
        Cmd::Analytics(AnalyticsCmd::Query { spec, r#async }) => {
            let spec = commands::analytics::load_spec(spec)?;
            commands::analytics::query(&ctx, &spec, *r#async)
        }
        Cmd::Analytics(AnalyticsCmd::Compare { spec, days }) => {
            let spec = commands::analytics::load_spec(spec)?;
            commands::analytics::compare(&ctx, &spec, *days, std::time::SystemTime::now())
        }
        Cmd::Jobs(JobsCmd::Get { job_id }) => commands::jobs::get(&ctx, job_id),
        Cmd::Jobs(JobsCmd::Wait { job_id, timeout }) => {
            commands::jobs::wait(&ctx, job_id, *timeout)
        }
        Cmd::Jobs(JobsCmd::Download {
            job_id,
            out,
            timeout,
        }) => commands::jobs::download(&ctx, job_id, out.as_deref(), *timeout),
        Cmd::Tools(cmd) => match cmd {
            ToolsCmd::List { full } => commands::tools::list(&ctx, *full),
            ToolsCmd::Describe { name } => commands::tools::describe(&ctx, name),
            ToolsCmd::Call {
                name,
                input,
                input_file,
                attrs,
                raw,
                out,
            } => commands::tools::call(
                &ctx,
                name,
                &commands::tools::CallOpts {
                    input: input.as_deref(),
                    input_file: input_file.as_deref(),
                    attrs,
                    raw: *raw,
                    out: out.as_deref(),
                },
            ),
        },
        Cmd::Status => commands::profiles::status(&ctx),
        Cmd::Mcp => {
            tokio::runtime::Runtime::new()
                .context("failed to start the async runtime")?
                .block_on(appcues::mcp::serve(ctx))?;
            Ok(String::new())
        }
        Cmd::Profiles(_) => unreachable!("handled before build_ctx"),
    }
}

fn main() {
    let cli = Cli::parse();
    match run(&cli) {
        Ok(out) => {
            if !out.is_empty() {
                println!("{out}");
            }
        }
        Err(e) => {
            let (code, line) = render_error(&e);
            eprintln!("{line}");
            std::process::exit(code);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use appcues::output::classify;
    use clap::CommandFactory;

    #[test]
    fn cli_definition_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn mcp_parses_as_a_bare_subcommand() {
        let cli = Cli::parse_from(["appcues", "mcp"]);
        assert!(matches!(cli.command, Cmd::Mcp));
    }

    #[test]
    fn flows_plus_digest_parses_with_days() {
        let cli = Cli::parse_from(["appcues", "flows", "+digest", "--days", "14"]);
        match cli.command {
            Cmd::Flows(FlowsCmd::Digest { days }) => assert_eq!(days, 14),
            _ => panic!("expected flows +digest to parse"),
        }
    }

    #[test]
    fn analytics_plus_compare_parses_spec_and_days() {
        let cli = Cli::parse_from([
            "appcues",
            "analytics",
            "+compare",
            "--spec",
            "q.json",
            "--days",
            "14",
        ]);
        match cli.command {
            Cmd::Analytics(AnalyticsCmd::Compare { spec, days }) => {
                assert_eq!(spec, "q.json");
                assert_eq!(days, 14);
            }
            _ => panic!("expected analytics +compare to parse"),
        }
    }

    #[test]
    fn analytics_plus_compare_rejects_days_over_90() {
        assert!(
            Cli::try_parse_from([
                "appcues",
                "analytics",
                "+compare",
                "--spec",
                "q.json",
                "--days",
                "91"
            ])
            .is_err()
        );
    }

    #[test]
    fn flows_plus_digest_rejects_days_over_90() {
        assert!(Cli::try_parse_from(["appcues", "flows", "+digest", "--days", "91"]).is_err());
    }

    #[test]
    fn experiences_type_values_parse_as_kebab_case() {
        assert!(Cli::try_parse_from(["appcues", "experiences", "list", "flows-v2"]).is_ok());
        assert!(Cli::try_parse_from(["appcues", "experiences", "get", "pins", "p1"]).is_ok());
        assert!(Cli::try_parse_from(["appcues", "experiences", "list", "nonsense"]).is_err());
    }

    #[test]
    fn classify_maps_errors_to_exit_codes() {
        let api = |status| {
            anyhow::Error::new(appcues::client::ApiError {
                status,
                message: "x".into(),
                body: None,
                rate_limit: None,
            })
        };
        assert_eq!(classify(&api(404)), (4, "api", Some(404)));
        assert_eq!(classify(&api(401)), (3, "auth", Some(401)));
        assert_eq!(classify(&api(403)), (3, "auth", Some(403)));
        assert_eq!(classify(&api(429)), (5, "rate_limited", Some(429)));
        assert_eq!(classify(&api(503)), (5, "server", Some(503)));
        let cfg = anyhow::Error::new(appcues::config::ConfigError("no key".into()));
        assert_eq!(classify(&cfg), (3, "config", None));
        assert_eq!(classify(&anyhow::anyhow!("boom")), (1, "unexpected", None));
        let tool = anyhow::Error::new(appcues::commands::tools::ToolError {
            tool: "create_campaign".into(),
            message: "objective_id not found".into(),
            envelope: serde_json::json!({"isError": true}),
        });
        assert_eq!(classify(&tool), (4, "tool", None));
    }

    #[test]
    fn tool_error_json_names_the_tool_and_embeds_the_envelope() {
        let e = anyhow::Error::new(appcues::commands::tools::ToolError {
            tool: "create_campaign".into(),
            message: "objective_id not found".into(),
            envelope: serde_json::json!({
                "content": [{"type": "text", "text": "objective_id not found"}],
                "isError": true
            }),
        });
        let (code, line) = render_error(&e);
        assert_eq!(code, 4);
        let v: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v["type"], "tool");
        assert_eq!(v["tool"], "create_campaign");
        assert_eq!(v["body"]["isError"], true);
        assert!(
            v["message"]
                .as_str()
                .unwrap()
                .contains("objective_id not found")
        );
    }

    #[test]
    fn tools_call_input_flags_are_mutually_exclusive() {
        assert!(
            Cli::try_parse_from([
                "appcues", "tools", "call", "t", "--input", "{}", "--attr", "a=1"
            ])
            .is_err()
        );
        assert!(
            Cli::try_parse_from([
                "appcues",
                "tools",
                "call",
                "t",
                "--input",
                "{}",
                "--input-file",
                "x.json"
            ])
            .is_err()
        );
        assert!(Cli::try_parse_from(["appcues", "tools", "call", "t", "--attr", "a=1"]).is_ok());
    }

    #[test]
    fn error_json_is_one_parseable_line() {
        let e = anyhow::Error::new(appcues::client::ApiError {
            status: 404,
            message: "flow not found".into(),
            body: None,
            rate_limit: None,
        });
        let (code, line) = render_error(&e);
        assert_eq!(code, 4);
        let v: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v["error"], true);
        assert_eq!(v["type"], "api");
        assert_eq!(v["status"], 404);
        assert!(v["message"].as_str().unwrap().contains("flow not found"));
    }

    #[test]
    fn error_json_embeds_structured_body_and_rate_limit() {
        let e = anyhow::Error::new(appcues::client::ApiError {
            status: 429,
            message: "API error 429: rate limited".into(),
            body: Some(serde_json::json!({"error": true, "title": "rate limited"})),
            rate_limit: Some(serde_json::json!({"retry_after": 30, "remaining": 0})),
        });
        let (code, line) = render_error(&e);
        assert_eq!(code, 5);
        let v: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v["body"]["title"], "rate limited");
        assert_eq!(v["rate_limit"]["retry_after"], 30);
    }
}
