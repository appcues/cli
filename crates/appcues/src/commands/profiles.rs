use super::{Ctx, confirm};
use crate::config::{
    ConfigError, ENV_VALUES, Env, Profile, list_profiles, remove_profile, write_profile,
};
use crate::output::{Format, render_list};
use anyhow::Result;
use serde_json::Value;
use std::io::Write;
use std::path::Path;

fn prompt(msg: &str) -> Result<String> {
    eprint!("{msg}");
    std::io::stderr().flush()?;
    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;
    Ok(line.trim().to_string())
}

/// Field values for `profiles add` and `profiles edit`. All optional:
/// add prompts for what's missing (on a TTY); edit keeps what's omitted.
pub struct ProfileArgs {
    pub api_key: Option<String>,
    pub api_secret: Option<String>,
    pub account_id: Option<String>,
    pub env: Option<String>,
    pub base_url: Option<String>,
    pub tools_base_url: Option<String>,
}

/// Fill each missing value: prompt on a TTY, otherwise fail with a
/// ConfigError naming the flag — the always-on tier has nobody to answer.
fn resolve_add(args: ProfileArgs, is_tty: bool) -> Result<Profile> {
    fn require(
        val: Option<String>,
        flag: &str,
        is_tty: bool,
        ask: impl FnOnce() -> Result<String>,
    ) -> Result<String> {
        let value = match val {
            Some(v) => Ok(v),
            None if is_tty => ask(),
            None => Err(anyhow::Error::from(ConfigError(format!(
                "--{flag} is required when stdin is not a terminal"
            )))),
        }?;
        if value.trim().is_empty() {
            return Err(ConfigError(format!("--{flag} must not be empty")).into());
        }
        Ok(value)
    }
    let api_key = require(args.api_key, "api-key", is_tty, || prompt("API key: "))?;
    let api_secret = require(args.api_secret, "api-secret", is_tty, || {
        Ok(rpassword::prompt_password("API secret: ")?)
    })?;
    let account_id = require(args.account_id, "account-id", is_tty, || {
        prompt("Account ID: ")
    })?;
    let env_input = match args.env {
        Some(e) => e,
        None if is_tty => prompt(&format!("Environment ({ENV_VALUES}) [prod]: "))?,
        None => String::new(),
    };
    let env: Env = if env_input.is_empty() {
        Env::Prod
    } else {
        env_input
            .parse()
            .map_err(|e: anyhow::Error| ConfigError(e.to_string()))?
    };
    Ok(Profile {
        api_key,
        api_secret,
        account_id,
        env,
        base_url: args.base_url.filter(|s| !s.is_empty()),
        tools_base_url: args.tools_base_url.filter(|s| !s.is_empty()),
        // Set by hand in config.toml (interactive = true) or per
        // invocation with -i; new profiles start non-interactive.
        interactive: false,
    })
}

pub fn add(config_path: &Path, profile_name: &str, args: ProfileArgs) -> Result<String> {
    use std::io::IsTerminal;
    if list_profiles(config_path)?.contains_key(profile_name) {
        return Err(ConfigError(format!(
            "profile '{profile_name}' already exists; use `appcues profiles edit {profile_name}`"
        ))
        .into());
    }
    let interactive = std::io::stdin().is_terminal();
    if interactive && args.api_key.is_none() {
        eprintln!("Create API credentials at studio.appcues.com → Settings → API Keys.");
    }
    let profile = resolve_add(args, interactive)?;
    write_profile(config_path, profile_name, &profile)?;
    Ok(format!(
        "Saved profile '{profile_name}' to {}.\nRun `appcues status` to verify.",
        config_path.display()
    ))
}

/// Update only the provided fields; everything omitted keeps its value.
/// `--base-url ""` clears the explicit URL (falling back to `env`).
pub fn edit(config_path: &Path, profile_name: &str, args: ProfileArgs) -> Result<String> {
    let mut profiles = list_profiles(config_path)?;
    let Some(mut p) = profiles.remove(profile_name) else {
        return Err(ConfigError(format!(
            "profile '{profile_name}' not found in {}",
            config_path.display()
        ))
        .into());
    };
    if let Some(v) = args.api_key {
        p.api_key = v;
    }
    if let Some(v) = args.api_secret {
        p.api_secret = v;
    }
    if let Some(v) = args.account_id {
        p.account_id = v;
    }
    if let Some(v) = args.env {
        p.env = v
            .parse()
            .map_err(|e: anyhow::Error| ConfigError(e.to_string()))?;
    }
    if let Some(v) = args.base_url {
        p.base_url = if v.is_empty() { None } else { Some(v) };
    }
    if let Some(v) = args.tools_base_url {
        p.tools_base_url = if v.is_empty() { None } else { Some(v) };
    }
    write_profile(config_path, profile_name, &p)?;
    Ok(format!(
        "Updated profile '{profile_name}' in {}.",
        config_path.display()
    ))
}

/// `interactive` is resolved by the caller (flag, APPCUES_INTERACTIVE,
/// or the active profile's field read from the config file alone).
pub fn remove(config_path: &Path, profile_name: &str, interactive: bool) -> Result<String> {
    if !list_profiles(config_path)?.contains_key(profile_name) {
        return Err(ConfigError(format!(
            "profile '{profile_name}' not found in {}",
            config_path.display()
        ))
        .into());
    }
    confirm(&format!("Remove profile '{profile_name}'?"), interactive)?;
    remove_profile(config_path, profile_name)?;
    Ok(format!(
        "Removed profile '{profile_name}' from {}.",
        config_path.display()
    ))
}

/// First 5 characters of a credential, enough to tell keys apart without
/// putting the secret on screen.
fn mask(s: &str) -> String {
    format!("{}…", s.chars().take(5).collect::<String>())
}

pub fn list(config_path: &Path, format: Format) -> Result<String> {
    let rows: Vec<Value> = list_profiles(config_path)?
        .into_iter()
        .map(|(name, p)| {
            let tools_url = p.tools_base_url().unwrap_or("").to_string();
            // Table: both URLs stacked in one cell. JSON keeps them as separate keys.
            let url = match format {
                Format::Table if !tools_url.is_empty() => format!("{}\n{tools_url}", p.base_url()),
                _ => p.base_url().to_string(),
            };
            serde_json::json!({
                "profile": name,
                "account_id": p.account_id,
                "env": p.env,
                "url": url,
                "tools_url": tools_url,
                "api_key": mask(&p.api_key),
                "api_secret": mask(&p.api_secret),
            })
        })
        .collect();
    Ok(render_list(
        &rows,
        format,
        &[
            "profile",
            "account_id",
            "env",
            "url",
            "api_key",
            "api_secret",
        ],
    ))
}

pub fn status(ctx: &Ctx) -> Result<String> {
    ctx.client.get(&ctx.path("tags"))?;
    Ok(format!(
        "Account {} via {}: credentials OK",
        ctx.account_id,
        ctx.client.base_url()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::load_profile;

    fn full_args() -> ProfileArgs {
        ProfileArgs {
            api_key: Some("test-key".into()),
            api_secret: Some("test-secret".into()),
            account_id: Some("acct1".into()),
            env: Some("prod-eu".into()),
            base_url: None,
            tools_base_url: None,
        }
    }

    #[test]
    fn all_flags_resolve_without_a_terminal() {
        let p = resolve_add(full_args(), false).unwrap();
        assert_eq!(p.api_key, "test-key");
        assert_eq!(p.env, Env::ProdEu);
    }

    #[test]
    fn missing_flag_without_terminal_is_a_config_error() {
        let mut args = full_args();
        args.api_secret = None;
        let err = resolve_add(args, false).unwrap_err();
        assert!(err.downcast_ref::<ConfigError>().is_some());
        assert!(err.to_string().contains("--api-secret"));
    }

    #[test]
    fn blank_required_values_are_rejected() {
        let mut args = full_args();
        args.api_key = Some("   ".into());
        let err = resolve_add(args, false).unwrap_err();
        assert!(err.downcast_ref::<ConfigError>().is_some());
        assert!(err.to_string().contains("--api-key"));
    }

    #[test]
    fn invalid_env_is_a_config_error() {
        let mut args = full_args();
        args.env = Some("mars".into());
        let err = resolve_add(args, false).unwrap_err();
        assert!(err.downcast_ref::<ConfigError>().is_some());
        assert!(err.to_string().contains("mars"));
    }

    #[test]
    fn env_defaults_to_prod_when_omitted_non_interactively() {
        let mut args = full_args();
        args.env = None;
        assert_eq!(resolve_add(args, false).unwrap().env, Env::Prod);
    }

    #[test]
    fn add_with_flags_writes_the_profile() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        add(&path, "ci", full_args()).unwrap();
        let p = load_profile(&path, "ci", &std::collections::HashMap::new()).unwrap();
        assert_eq!(p.account_id, "acct1");
    }

    #[test]
    fn add_refuses_an_existing_profile_and_points_at_edit() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        add(&path, "ci", full_args()).unwrap();
        let err = add(&path, "ci", full_args()).unwrap_err();
        assert!(err.downcast_ref::<ConfigError>().is_some());
        assert!(err.to_string().contains("profiles edit ci"));
    }

    #[test]
    fn edit_updates_only_the_provided_fields() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        add(&path, "ci", full_args()).unwrap();
        edit(
            &path,
            "ci",
            ProfileArgs {
                api_key: None,
                api_secret: None,
                account_id: Some("acct2".into()),
                env: Some("prod-eu".into()),
                base_url: None,
                tools_base_url: None,
            },
        )
        .unwrap();
        let p = load_profile(&path, "ci", &std::collections::HashMap::new()).unwrap();
        assert_eq!(p.account_id, "acct2");
        assert_eq!(p.env, Env::ProdEu);
        assert_eq!(p.api_key, "test-key"); // untouched
        assert_eq!(p.api_secret, "test-secret"); // untouched
    }

    #[test]
    fn edit_sets_and_clears_base_url() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        add(&path, "ci", full_args()).unwrap();
        let args = |base_url: &str| ProfileArgs {
            api_key: None,
            api_secret: None,
            account_id: None,
            env: None,
            base_url: Some(base_url.into()),
            tools_base_url: None,
        };
        edit(&path, "ci", args("http://localhost:4000")).unwrap();
        let p = load_profile(&path, "ci", &std::collections::HashMap::new()).unwrap();
        assert_eq!(p.base_url(), "http://localhost:4000");
        edit(&path, "ci", args("")).unwrap(); // empty clears, env wins again
        let p = load_profile(&path, "ci", &std::collections::HashMap::new()).unwrap();
        assert_eq!(p.base_url(), "https://api.eu.appcues.com");
    }

    #[test]
    fn edit_of_a_missing_profile_is_a_config_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let err = edit(&path, "nope", full_args()).unwrap_err();
        assert!(err.downcast_ref::<ConfigError>().is_some());
        assert!(err.to_string().contains("nope"));
    }

    #[test]
    fn remove_deletes_only_the_named_profile() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        add(&path, "ci", full_args()).unwrap();
        add(&path, "other", full_args()).unwrap();
        remove(&path, "ci", false).unwrap();
        let profiles = list_profiles(&path).unwrap();
        assert!(!profiles.contains_key("ci"));
        assert!(profiles.contains_key("other"));
    }

    #[test]
    fn remove_of_a_missing_profile_is_a_config_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let err = remove(&path, "nope", false).unwrap_err();
        assert!(err.downcast_ref::<ConfigError>().is_some());
    }

    #[test]
    fn list_shows_profiles_with_truncated_credentials() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        add(&path, "ci", full_args()).unwrap();
        let out = list(&path, Format::Table).unwrap();
        assert!(out.contains("ci"));
        assert!(out.contains("acct1"));
        assert!(out.contains("prod-eu"));
        assert!(out.contains("https://api.eu.appcues.com"));
        assert!(out.contains("test-…"));
        assert!(!out.contains("test-key") && !out.contains("test-secret"));
    }

    #[test]
    fn list_json_masks_credentials() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        add(&path, "ci", full_args()).unwrap();
        let out = list(&path, Format::Json).unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v[0]["profile"], "ci");
        assert_eq!(v[0]["api_key"], "test-…");
        assert_eq!(v[0]["api_secret"], "test-…");
        assert_eq!(v[0]["env"], "prod-eu");
    }

    #[test]
    fn list_with_no_config_is_empty_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let out = list(&dir.path().join("missing-config.toml"), Format::Json).unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v.as_array().unwrap().len(), 0);
    }
}
