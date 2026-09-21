use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::path::Path;

/// A configuration/credentials problem (bad TOML, unknown profile,
/// missing fields). main.rs downcasts to exit 3.
#[derive(Debug)]
pub struct ConfigError(pub String);

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl std::error::Error for ConfigError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Env {
    #[default]
    Prod,
    ProdEu,
}

pub const ENV_VALUES: &str = "prod, prod-eu";

impl Env {
    pub fn base_url(&self) -> &'static str {
        match self {
            Env::Prod => "https://api.appcues.com",
            Env::ProdEu => "https://api.eu.appcues.com",
        }
    }

    /// The tools (mcp facet) origin for this environment.
    pub fn tools_base_url(&self) -> Option<&'static str> {
        match self {
            Env::Prod => Some("https://mcp.appcues.com"),
            Env::ProdEu => Some("https://mcp.eu.appcues.com"),
        }
    }
}

impl std::str::FromStr for Env {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self> {
        match s.to_ascii_lowercase().as_str() {
            "prod" => Ok(Env::Prod),
            "prod-eu" => Ok(Env::ProdEu),
            other => bail!("unknown env '{other}' (expected one of: {ENV_VALUES})"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub api_key: String,
    pub api_secret: String,
    pub account_id: String,
    #[serde(default)]
    pub env: Env,
    /// Explicit API origin; when set it wins over `env`'s URL. The escape
    /// hatch for localhost, proxies, or hostnames the CLI doesn't know.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    /// Explicit tools origin; when set it wins over `env`'s tools URL.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools_base_url: Option<String>,
    /// When true, destructive commands prompt y/N on a terminal instead of
    /// running straight through. Off by default: the CLI is agent-first,
    /// and safety is delegated to API key permissions. The `-i` flag turns
    /// it on per invocation; APPCUES_INTERACTIVE overrides the file.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub interactive: bool,
}

impl Profile {
    /// The API origin this profile talks to: explicit base_url if set,
    /// otherwise the named environment's URL.
    pub fn base_url(&self) -> &str {
        self.base_url.as_deref().unwrap_or(self.env.base_url())
    }

    /// The tools origin this profile talks to: explicit override if set,
    /// otherwise the named environment's tools URL.
    pub fn tools_base_url(&self) -> Option<&str> {
        self.tools_base_url
            .as_deref()
            .or_else(|| self.env.tools_base_url())
    }
}

#[derive(Default, Serialize, Deserialize)]
struct ConfigFile {
    #[serde(default)]
    profiles: BTreeMap<String, Profile>,
}

fn read_config(path: &Path) -> Result<ConfigFile> {
    if !path.exists() {
        return Ok(ConfigFile::default());
    }
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    toml::from_str(&text).map_err(|e| {
        let mut msg = format!("invalid TOML in {}: {e}", path.display());
        // Unknown keys are rejected (deny_unknown_fields) so a legacy or
        // mistyped field fails loudly instead of being silently ignored;
        // give the one known legacy key a migration hint.
        if msg.contains("unknown field `region`") {
            msg.push_str("; `region` was replaced by `env` (us -> prod, eu -> prod-eu)");
        }
        ConfigError(msg).into()
    })
}

/// All saved profiles, sorted by name. A missing config file is an empty map.
pub fn list_profiles(config_path: &Path) -> Result<BTreeMap<String, Profile>> {
    Ok(read_config(config_path)?.profiles)
}

pub fn load_profile(
    config_path: &Path,
    name: &str,
    env: &HashMap<String, String>,
) -> Result<Profile> {
    let config = read_config(config_path)?;
    let file_profile = config.profiles.get(name).cloned();
    if file_profile.is_none() && config_path.exists() && env.get("APPCUES_API_KEY").is_none() {
        return Err(ConfigError(format!(
            "profile '{name}' not found in {}",
            config_path.display()
        ))
        .into());
    }
    let field = |env_key: &str, file_val: Option<String>, what: &str| -> Result<String> {
        env.get(env_key).cloned().or(file_val).ok_or_else(|| {
            ConfigError(format!(
                "no {what} configured: set {env_key} or run `appcues profiles add`"
            ))
            .into()
        })
    };
    let (fk, fs, fa, fe, fb, ft, fi) = match file_profile {
        Some(p) => (
            Some(p.api_key),
            Some(p.api_secret),
            Some(p.account_id),
            Some(p.env),
            p.base_url,
            p.tools_base_url,
            Some(p.interactive),
        ),
        None => (None, None, None, None, None, None, None),
    };
    Ok(Profile {
        api_key: field("APPCUES_API_KEY", fk, "API key")?,
        api_secret: field("APPCUES_API_SECRET", fs, "API secret")?,
        account_id: field("APPCUES_ACCOUNT_ID", fa, "account ID")?,
        env: match env.get("APPCUES_ENV") {
            Some(s) => s.parse().map_err(|e| ConfigError(format!("{e}")))?,
            None => fe.unwrap_or_default(),
        },
        base_url: absolute_http_url(env.get("APPCUES_BASE_URL").cloned().or(fb), "base_url")?,
        tools_base_url: absolute_http_url(
            env.get("APPCUES_TOOLS_BASE_URL").cloned().or(ft),
            "tools_base_url",
        )?,
        interactive: match env.get("APPCUES_INTERACTIVE") {
            Some(s) => parse_interactive(s)?,
            None => fi.unwrap_or_default(),
        },
    })
}

/// Parse an APPCUES_INTERACTIVE value; anything but true/false is a
/// ConfigError. Shared with paths that resolve interactivity without
/// loading a full profile (`profiles remove`).
pub fn parse_interactive(s: &str) -> Result<bool> {
    s.parse().map_err(|_| {
        ConfigError(format!(
            "invalid APPCUES_INTERACTIVE '{s}': expected true or false"
        ))
        .into()
    })
}

/// An explicit origin override must be an absolute http(s) URL with a
/// host; anything else (empty, bare scheme, missing host) is a
/// ConfigError naming the field, so it fails at exit 3 instead of as a
/// request error later.
fn absolute_http_url(value: Option<String>, what: &str) -> Result<Option<String>> {
    let Some(u) = value else { return Ok(None) };
    let host = u
        .strip_prefix("http://")
        .or_else(|| u.strip_prefix("https://"))
        .map(|rest| rest.split('/').next().unwrap_or(""));
    match host {
        Some(h) if !h.is_empty() => Ok(Some(u)),
        _ => Err(ConfigError(format!(
            "invalid {what} '{u}': must be an absolute http(s) URL with a host"
        ))
        .into()),
    }
}

pub fn write_profile(config_path: &Path, name: &str, profile: &Profile) -> Result<()> {
    let mut config = read_config(config_path)?;
    config.profiles.insert(name.to_string(), profile.clone());
    save_config(config_path, &config)
}

pub fn remove_profile(config_path: &Path, name: &str) -> Result<()> {
    let mut config = read_config(config_path)?;
    if config.profiles.remove(name).is_none() {
        return Err(ConfigError(format!(
            "profile '{name}' not found in {}",
            config_path.display()
        ))
        .into());
    }
    save_config(config_path, &config)
}

fn save_config(config_path: &Path, config: &ConfigFile) -> Result<()> {
    let parent = match config_path.parent() {
        Some(parent) => {
            std::fs::create_dir_all(parent)?;
            parent
        }
        None => Path::new("."),
    };
    let tmp_path = parent.join(format!(
        ".{}.tmp",
        config_path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "config".to_string())
    ));
    let contents = toml::to_string_pretty(config)?;

    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp_path)
            .with_context(|| format!("failed to create {}", tmp_path.display()))?;
        file.write_all(contents.as_bytes())?;
    }
    #[cfg(not(unix))]
    {
        std::fs::write(&tmp_path, &contents)
            .with_context(|| format!("failed to write {}", tmp_path.display()))?;
    }

    std::fs::rename(&tmp_path, config_path)
        .with_context(|| format!("failed to write {}", config_path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn env(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn loads_profile_from_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            "[profiles.default]\napi_key = \"test-key\"\napi_secret = \"test-secret\"\naccount_id = \"acct1\"\nenv = \"prod-eu\"\n",
        )
        .unwrap();
        let p = load_profile(&path, "default", &env(&[])).unwrap();
        assert_eq!(p.api_key, "test-key");
        assert_eq!(p.env, Env::ProdEu);
        assert_eq!(p.base_url(), "https://api.eu.appcues.com");
    }

    #[test]
    fn env_overrides_file_per_field() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            "[profiles.default]\napi_key = \"test-key\"\napi_secret = \"test-secret\"\naccount_id = \"acct1\"\n",
        )
        .unwrap();
        let p = load_profile(&path, "default", &env(&[("APPCUES_API_KEY", "env-key")])).unwrap();
        assert_eq!(p.api_key, "env-key");
        assert_eq!(p.api_secret, "test-secret");
        assert_eq!(p.env, Env::Prod); // default when absent
        assert_eq!(p.base_url(), "https://api.appcues.com");
    }

    /// A config path guaranteed absent: a missing child of a fresh tempdir.
    fn missing_config() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("missing-config.toml");
        (dir, path)
    }

    #[test]
    fn env_only_needs_no_file() {
        let (_dir, path) = missing_config();
        let p = load_profile(
            &path,
            "default",
            &env(&[
                ("APPCUES_API_KEY", "k"),
                ("APPCUES_API_SECRET", "s"),
                ("APPCUES_ACCOUNT_ID", "a"),
                ("APPCUES_ENV", "prod-eu"),
            ]),
        )
        .unwrap();
        assert_eq!(p.env, Env::ProdEu);
        assert_eq!(p.base_url(), "https://api.eu.appcues.com");
    }

    #[test]
    fn file_base_url_wins_over_named_env() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            "[profiles.local]\napi_key = \"test-key\"\napi_secret = \"test-secret\"\naccount_id = \"acct1\"\nenv = \"prod-eu\"\nbase_url = \"http://localhost:4000\"\n",
        )
        .unwrap();
        let p = load_profile(&path, "local", &env(&[])).unwrap();
        assert_eq!(p.base_url(), "http://localhost:4000");
    }

    #[test]
    fn base_url_env_var_wins_over_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            "[profiles.default]\napi_key = \"test-key\"\napi_secret = \"test-secret\"\naccount_id = \"acct1\"\nbase_url = \"http://localhost:4000\"\n",
        )
        .unwrap();
        let p = load_profile(
            &path,
            "default",
            &env(&[("APPCUES_BASE_URL", "http://localhost:9999")]),
        )
        .unwrap();
        assert_eq!(p.base_url(), "http://localhost:9999");
    }

    #[test]
    fn tools_base_url_maps_envs() {
        assert_eq!(Env::Prod.tools_base_url(), Some("https://mcp.appcues.com"));
        assert_eq!(
            Env::ProdEu.tools_base_url(),
            Some("https://mcp.eu.appcues.com")
        );
    }

    #[test]
    fn tools_base_url_env_var_beats_profile_beats_env_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            "[profiles.default]\napi_key = \"test-key\"\napi_secret = \"test-secret\"\naccount_id = \"acct1\"\ntools_base_url = \"http://localhost:5000\"\n",
        )
        .unwrap();
        // profile override beats the env default
        let p = load_profile(&path, "default", &env(&[])).unwrap();
        assert_eq!(p.tools_base_url(), Some("http://localhost:5000"));
        // env var beats the profile override
        let p = load_profile(
            &path,
            "default",
            &env(&[("APPCUES_TOOLS_BASE_URL", "http://localhost:9999")]),
        )
        .unwrap();
        assert_eq!(p.tools_base_url(), Some("http://localhost:9999"));
    }

    #[test]
    fn tools_base_url_defaults_from_the_named_env() {
        let (_dir, path) = missing_config();
        let p = load_profile(
            &path,
            "default",
            &env(&[
                ("APPCUES_API_KEY", "k"),
                ("APPCUES_API_SECRET", "s"),
                ("APPCUES_ACCOUNT_ID", "a"),
                ("APPCUES_ENV", "prod-eu"),
            ]),
        )
        .unwrap();
        assert_eq!(p.tools_base_url(), Some("https://mcp.eu.appcues.com"));
    }

    #[test]
    fn interactive_defaults_off_and_reads_from_file_and_env() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            "[profiles.default]\napi_key = \"test-key\"\napi_secret = \"test-secret\"\naccount_id = \"acct1\"\n",
        )
        .unwrap();
        // default off
        assert!(
            !load_profile(&path, "default", &env(&[]))
                .unwrap()
                .interactive
        );
        // env var turns it on
        assert!(
            load_profile(&path, "default", &env(&[("APPCUES_INTERACTIVE", "true")]))
                .unwrap()
                .interactive
        );
        // file field turns it on; env var can force it back off
        std::fs::write(
            &path,
            "[profiles.default]\napi_key = \"test-key\"\napi_secret = \"test-secret\"\naccount_id = \"acct1\"\ninteractive = true\n",
        )
        .unwrap();
        assert!(
            load_profile(&path, "default", &env(&[]))
                .unwrap()
                .interactive
        );
        assert!(
            !load_profile(&path, "default", &env(&[("APPCUES_INTERACTIVE", "false")]))
                .unwrap()
                .interactive
        );
    }

    #[test]
    fn invalid_interactive_env_var_is_a_config_error() {
        let (_dir, path) = missing_config();
        let err = load_profile(
            &path,
            "default",
            &env(&[
                ("APPCUES_API_KEY", "k"),
                ("APPCUES_API_SECRET", "s"),
                ("APPCUES_ACCOUNT_ID", "a"),
                ("APPCUES_INTERACTIVE", "maybe"),
            ]),
        )
        .unwrap_err();
        assert!(err.downcast_ref::<ConfigError>().is_some());
        assert!(err.to_string().contains("APPCUES_INTERACTIVE"));
    }

    #[test]
    fn scheme_without_host_is_a_config_error() {
        let (_dir, path) = missing_config();
        for bad in ["http://", "https://", "http:///path"] {
            let err = load_profile(
                &path,
                "default",
                &env(&[
                    ("APPCUES_API_KEY", "k"),
                    ("APPCUES_API_SECRET", "s"),
                    ("APPCUES_ACCOUNT_ID", "a"),
                    ("APPCUES_BASE_URL", bad),
                ]),
            )
            .unwrap_err();
            assert!(err.downcast_ref::<ConfigError>().is_some(), "for {bad}");
            assert!(err.to_string().contains("host"), "for {bad}");
        }
    }

    #[test]
    fn malformed_tools_base_url_is_a_config_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            "[profiles.default]\napi_key = \"test-key\"\napi_secret = \"test-secret\"\naccount_id = \"acct1\"\ntools_base_url = \"localhost:5000\"\n",
        )
        .unwrap();
        let err = load_profile(&path, "default", &env(&[])).unwrap_err();
        assert!(err.downcast_ref::<ConfigError>().is_some());
        assert!(err.to_string().contains("tools_base_url"));
    }

    #[test]
    fn invalid_env_is_a_config_error() {
        let (_dir, path) = missing_config();
        let err = load_profile(
            &path,
            "default",
            &env(&[
                ("APPCUES_API_KEY", "k"),
                ("APPCUES_API_SECRET", "s"),
                ("APPCUES_ACCOUNT_ID", "a"),
                ("APPCUES_ENV", "mars"),
            ]),
        )
        .unwrap_err();
        assert!(err.downcast_ref::<ConfigError>().is_some());
        assert!(err.to_string().contains("mars"));
    }

    #[test]
    fn legacy_region_key_is_rejected_with_a_migration_hint() {
        // Pre-env configs carried region = "us"|"eu". A silent fallback to
        // prod would send an EU profile to the US API, so reject instead.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            "[profiles.default]\napi_key = \"test-key\"\napi_secret = \"test-secret\"\naccount_id = \"acct1\"\nregion = \"eu\"\n",
        )
        .unwrap();
        let err = load_profile(&path, "default", &env(&[])).unwrap_err();
        assert!(err.downcast_ref::<ConfigError>().is_some());
        let msg = err.to_string();
        assert!(
            msg.contains("region") && msg.contains("prod-eu"),
            "got: {msg}"
        );
    }

    #[test]
    fn unknown_profile_keys_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            "[profiles.default]\napi_key = \"test-key\"\napi_secret = \"test-secret\"\naccount_id = \"acct1\"\nbase_ur = \"typo\"\n",
        )
        .unwrap();
        let err = load_profile(&path, "default", &env(&[])).unwrap_err();
        assert!(err.downcast_ref::<ConfigError>().is_some());
        assert!(err.to_string().contains("base_ur"));
    }

    #[test]
    fn malformed_base_url_in_file_is_a_config_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            "[profiles.default]\napi_key = \"test-key\"\napi_secret = \"test-secret\"\naccount_id = \"acct1\"\nbase_url = \"localhost:4000\"\n",
        )
        .unwrap();
        let err = load_profile(&path, "default", &env(&[])).unwrap_err();
        assert!(err.downcast_ref::<ConfigError>().is_some());
        assert!(err.to_string().contains("localhost:4000"));
    }

    #[test]
    fn empty_base_url_env_var_is_a_config_error() {
        let dir = tempfile::tempdir().unwrap();
        let err = load_profile(
            &dir.path().join("missing-config.toml"),
            "default",
            &env(&[
                ("APPCUES_API_KEY", "k"),
                ("APPCUES_API_SECRET", "s"),
                ("APPCUES_ACCOUNT_ID", "a"),
                ("APPCUES_BASE_URL", ""),
            ]),
        )
        .unwrap_err();
        assert!(err.downcast_ref::<ConfigError>().is_some());
        assert!(err.to_string().contains("base_url"));
    }

    #[test]
    fn missing_credentials_point_to_profiles_add() {
        let (_dir, path) = missing_config();
        let err = load_profile(&path, "default", &env(&[])).unwrap_err();
        assert!(err.to_string().contains("appcues profiles add"));
    }

    #[test]
    fn unknown_profile_is_a_clear_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "[profiles.default]\napi_key = \"test-key\"\napi_secret = \"test-secret\"\naccount_id = \"acct1\"\n").unwrap();
        let err = load_profile(&path, "staging", &env(&[])).unwrap_err();
        assert!(err.to_string().contains("staging"));
    }

    #[test]
    fn unknown_profile_is_a_config_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "[profiles.default]\napi_key = \"test-key\"\napi_secret = \"test-secret\"\naccount_id = \"acct1\"\n").unwrap();
        let err = load_profile(&path, "staging", &env(&[])).unwrap_err();
        assert!(err.downcast_ref::<ConfigError>().is_some());
    }

    #[test]
    fn missing_credentials_is_a_config_error() {
        let (_dir, path) = missing_config();
        let err = load_profile(&path, "default", &env(&[])).unwrap_err();
        assert!(err.downcast_ref::<ConfigError>().is_some());
    }

    #[test]
    fn write_profile_roundtrips_and_sets_0600() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/config.toml");
        let p = Profile {
            api_key: "test-key".into(),
            api_secret: "test-secret".into(),
            account_id: "acct1".into(),
            env: Env::Prod,
            base_url: None,
            tools_base_url: None,
            interactive: false,
        };
        write_profile(&path, "default", &p).unwrap();
        let loaded = load_profile(&path, "default", &HashMap::new()).unwrap();
        assert_eq!(loaded, p);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }

    #[test]
    fn write_profile_preserves_other_profiles() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let a = Profile {
            api_key: "test-key".into(),
            api_secret: "test-secret".into(),
            account_id: "acct1".into(),
            env: Env::Prod,
            base_url: None,
            tools_base_url: None,
            interactive: false,
        };
        let b = Profile {
            api_key: "test-key2".into(),
            api_secret: "test-secret2".into(),
            account_id: "acct2".into(),
            env: Env::ProdEu,
            base_url: None,
            tools_base_url: None,
            interactive: false,
        };
        write_profile(&path, "default", &a).unwrap();
        write_profile(&path, "eu", &b).unwrap();
        assert_eq!(load_profile(&path, "default", &HashMap::new()).unwrap(), a);
        assert_eq!(load_profile(&path, "eu", &HashMap::new()).unwrap(), b);
        // Overwriting via the temp-file-then-rename path must still leave
        // the config at 0600 (no window where it's world/group readable).
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }
}
