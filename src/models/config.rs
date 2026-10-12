use crate::interactions::nubby::NUBBY_ANNOUNCE_RESET_HOURS;
use crate::mtg::providers::{ArchidektClient, MoxfieldClient, ProviderClients};
use chrono::{Duration, Utc};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use std::collections::HashSet;
use std::fmt;
use std::num::NonZeroU64;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum MTGCollectionProvider {
    Archidekt,
    Moxfield,
}

impl fmt::Display for MTGCollectionProvider {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            MTGCollectionProvider::Archidekt => write!(f, "archidekt"),
            MTGCollectionProvider::Moxfield => write!(f, "moxfield"),
        }
    }
}

#[derive(Deserialize)]
pub struct MTGCollectionConfig {
    pub provider: MTGCollectionProvider,
    pub discord_user: String,
    pub provider_collection: String,
}

#[derive(Deserialize)]
pub struct MTGCommunityDeck {
    pub provider: MTGCollectionProvider,
    pub discord_user: String,
    pub provider_deck: String,
}

#[derive(Deserialize)]
pub struct MTGConfig {
    pub collections: Vec<MTGCollectionConfig>,
    pub community_decks: Vec<MTGCommunityDeck>,
}

#[derive(Deserialize)]
pub struct NubbyConfig {
    pub nubby_user_id: u64,
    #[serde(skip_deserializing, default = "default_last_announced")]
    pub nubby_last_announced: Mutex<chrono::DateTime<Utc>>,
}

fn default_last_announced() -> Mutex<chrono::DateTime<Utc>> {
    let nubby_last_announced: Mutex<chrono::DateTime<Utc>> =
        Mutex::new(Utc::now() - Duration::hours(NUBBY_ANNOUNCE_RESET_HOURS + 1));
    nubby_last_announced
}

#[derive(Deserialize)]
pub struct CommonConfig {
    pub general_channel_id: u64,
}

/// Values read from the process environment once at startup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvConfig {
    /// Guild the slash commands are registered in (`GUILD_ID`).
    pub guild_id: u64,
    /// User-Agent sent to the Moxfield API (`MOXFIELD_USER_AGENT`).
    pub moxfield_user_agent: String,
}

impl EnvConfig {
    /// Read the required variables through `lookup` (e.g. `std::env::var(..).ok()`).
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Result<Self, ConfigError> {
        let required = |name: &'static str| {
            lookup(name)
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
                .ok_or(ConfigError::MissingEnv(name))
        };

        let guild_id = required("GUILD_ID")?
            .parse::<NonZeroU64>()
            .map_err(|e| ConfigError::InvalidEnv {
                name: "GUILD_ID",
                reason: format!("must be a non-zero integer ({e})"),
            })?
            .get();
        let moxfield_user_agent = required("MOXFIELD_USER_AGENT")?;

        Ok(EnvConfig {
            guild_id,
            moxfield_user_agent,
        })
    }

    /// Read the required variables from the process environment.
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_lookup(|name| std::env::var(name).ok())
    }
}

pub struct BotConfig {
    pub mtg: MTGConfig,
    pub nubby: NubbyConfig,
    pub common: CommonConfig,
    pub env: EnvConfig,
    /// Provider HTTP clients, built once from `env`.
    pub providers: ProviderClients,
}

#[derive(Debug)]
pub enum ConfigError {
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    Parse {
        path: PathBuf,
        source: serde_yaml::Error,
    },
    MissingEnv(&'static str),
    InvalidEnv {
        name: &'static str,
        reason: String,
    },
    Invalid(String),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ConfigError::Io { path, source } => {
                write!(f, "could not open '{}': {}", path.display(), source)
            }
            ConfigError::Parse { path, source } => {
                write!(f, "could not parse '{}': {}", path.display(), source)
            }
            ConfigError::MissingEnv(name) => write!(f, "expected {name} in the environment"),
            ConfigError::InvalidEnv { name, reason } => write!(f, "invalid {name}: {reason}"),
            ConfigError::Invalid(msg) => write!(f, "invalid config: {msg}"),
        }
    }
}

impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ConfigError::Io { source, .. } => Some(source),
            ConfigError::Parse { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// Check the MTG config for empty/padded IDs and duplicate `(provider, id)` pairs.
pub fn validate_mtg_config(mtg: &MTGConfig) -> Result<(), ConfigError> {
    let collections = mtg
        .collections
        .iter()
        .map(|c| (c.provider, c.provider_collection.as_str()));
    let decks = mtg
        .community_decks
        .iter()
        .map(|d| (d.provider, d.provider_deck.as_str()));
    validate_ids("collection", collections)?;
    validate_ids("community deck", decks)
}

fn validate_ids<'a>(
    kind: &str,
    ids: impl Iterator<Item = (MTGCollectionProvider, &'a str)>,
) -> Result<(), ConfigError> {
    let mut seen = HashSet::new();
    for (provider, id) in ids {
        if id.is_empty() {
            return Err(ConfigError::Invalid(format!("empty {provider} {kind} id")));
        }
        if id.trim() != id {
            return Err(ConfigError::Invalid(format!(
                "{provider} {kind} id {id:?} has surrounding whitespace"
            )));
        }
        if !seen.insert((provider, id)) {
            return Err(ConfigError::Invalid(format!(
                "duplicate {provider} {kind} id {id:?}"
            )));
        }
    }
    Ok(())
}

fn load_yaml<T: DeserializeOwned>(dir: &Path, file: &str) -> Result<T, ConfigError> {
    let path = dir.join(file);
    let f = std::fs::File::open(&path).map_err(|source| ConfigError::Io {
        path: path.clone(),
        source,
    })?;
    serde_yaml::from_reader(f).map_err(|source| ConfigError::Parse { path, source })
}

/// Build the provider HTTP clients; the Moxfield client sends `env.moxfield_user_agent`.
pub fn build_provider_clients(env: &EnvConfig) -> Result<ProviderClients, ConfigError> {
    Ok(ProviderClients {
        archidekt: ArchidektClient::new()
            .map_err(|e| ConfigError::Invalid(format!("archidekt client: {e}")))?,
        moxfield: MoxfieldClient::new(&env.moxfield_user_agent).map_err(|e| {
            ConfigError::InvalidEnv {
                name: "MOXFIELD_USER_AGENT",
                reason: e.to_string(),
            }
        })?,
    })
}

/// Load and validate `mtg.yaml`, `common.yaml` and `nubby.yaml` from `dir`.
pub fn load_config_from(dir: &Path, env: EnvConfig) -> Result<BotConfig, ConfigError> {
    let mtg: MTGConfig = load_yaml(dir, "mtg.yaml")?;
    validate_mtg_config(&mtg)?;
    Ok(BotConfig {
        mtg,
        common: load_yaml(dir, "common.yaml")?,
        nubby: load_yaml(dir, "nubby.yaml")?,
        providers: build_provider_clients(&env)?,
        env,
    })
}

/// Startup entry point: reads the environment and `config/` (relative to the
/// working directory), panicking with a clear message if anything is wrong.
pub fn load_config() -> BotConfig {
    EnvConfig::from_env()
        .and_then(|env| load_config_from(Path::new("config"), env))
        .unwrap_or_else(|e| {
            log::error!("Startup config error: {e}");
            panic!("Startup config error: {e}")
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn lookup(vars: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = vars
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |name| map.get(name).cloned()
    }

    fn parse_collection(yaml: &str) -> Result<MTGCollectionConfig, serde_yaml::Error> {
        serde_yaml::from_str(yaml)
    }

    fn mtg(yaml: &str) -> MTGConfig {
        serde_yaml::from_str(yaml).unwrap()
    }

    #[test]
    fn lowercase_provider_names_parse() {
        let a = parse_collection("{provider: archidekt, discord_user: a, provider_collection: x}")
            .unwrap();
        let m = parse_collection("{provider: moxfield, discord_user: a, provider_collection: x}")
            .unwrap();
        assert_eq!(a.provider, MTGCollectionProvider::Archidekt);
        assert_eq!(m.provider, MTGCollectionProvider::Moxfield);
    }

    #[test]
    fn capitalised_provider_name_fails() {
        for name in ["Archidekt", "MOXFIELD", "Moxfield"] {
            let yaml = format!("{{provider: {name}, discord_user: a, provider_collection: x}}");
            assert!(parse_collection(&yaml).is_err(), "{name} should not parse");
        }
    }

    #[test]
    fn unknown_provider_name_fails() {
        let yaml = "{provider: tappedout, discord_user: a, provider_collection: x}";
        assert!(parse_collection(yaml).is_err());
    }

    #[test]
    fn numeric_collection_id_deserializes_as_string() {
        let c =
            parse_collection("{provider: archidekt, discord_user: a, provider_collection: 243732}")
                .unwrap();
        assert_eq!(c.provider_collection, "243732");

        let d: MTGCommunityDeck =
            serde_yaml::from_str("{provider: archidekt, discord_user: a, provider_deck: 3531305}")
                .unwrap();
        assert_eq!(d.provider_deck, "3531305");
    }

    #[test]
    fn nubby_last_announced_defaults_to_about_13_hours_ago() {
        let nubby: NubbyConfig = serde_yaml::from_str("nubby_user_id: 42").unwrap();
        assert_eq!(nubby.nubby_user_id, 42);
        let last = *nubby.nubby_last_announced.lock().unwrap();
        let age = Utc::now() - last;
        let expected = Duration::hours(NUBBY_ANNOUNCE_RESET_HOURS + 1);
        assert_eq!(expected, Duration::hours(13));
        assert!(
            (age - expected).num_seconds().abs() < 5,
            "expected age ~{expected}, got {age}"
        );
    }

    #[test]
    fn env_config_reads_required_vars() {
        let env = EnvConfig::from_lookup(lookup(&[
            ("GUILD_ID", "1223050445128466553"),
            ("MOXFIELD_USER_AGENT", "test-agent/1.0"),
        ]))
        .unwrap();
        assert_eq!(
            env,
            EnvConfig {
                guild_id: 1223050445128466553,
                moxfield_user_agent: "test-agent/1.0".to_string(),
            }
        );
    }

    #[test]
    fn env_config_missing_moxfield_user_agent_fails() {
        let err = EnvConfig::from_lookup(lookup(&[("GUILD_ID", "1")])).unwrap_err();
        assert!(matches!(
            err,
            ConfigError::MissingEnv("MOXFIELD_USER_AGENT")
        ));
        assert!(err.to_string().contains("MOXFIELD_USER_AGENT"));
    }

    #[test]
    fn env_config_blank_moxfield_user_agent_fails() {
        let err =
            EnvConfig::from_lookup(lookup(&[("GUILD_ID", "1"), ("MOXFIELD_USER_AGENT", "  ")]))
                .unwrap_err();
        assert!(matches!(
            err,
            ConfigError::MissingEnv("MOXFIELD_USER_AGENT")
        ));
    }

    #[test]
    fn env_config_missing_guild_id_fails() {
        let err = EnvConfig::from_lookup(lookup(&[("MOXFIELD_USER_AGENT", "ua")])).unwrap_err();
        assert!(matches!(err, ConfigError::MissingEnv("GUILD_ID")));
    }

    #[test]
    fn env_config_non_numeric_guild_id_fails() {
        let err = EnvConfig::from_lookup(lookup(&[
            ("GUILD_ID", "not-a-number"),
            ("MOXFIELD_USER_AGENT", "ua"),
        ]))
        .unwrap_err();
        assert!(matches!(
            err,
            ConfigError::InvalidEnv {
                name: "GUILD_ID",
                ..
            }
        ));
    }

    #[test]
    fn env_config_zero_guild_id_fails() {
        let err =
            EnvConfig::from_lookup(lookup(&[("GUILD_ID", "0"), ("MOXFIELD_USER_AGENT", "ua")]))
                .unwrap_err();
        assert!(matches!(
            err,
            ConfigError::InvalidEnv {
                name: "GUILD_ID",
                ..
            }
        ));
    }

    #[test]
    fn provider_clients_build_from_env() {
        let env = EnvConfig {
            guild_id: 1,
            moxfield_user_agent: "test-agent/1.0".to_string(),
        };
        assert!(build_provider_clients(&env).is_ok());
    }

    #[test]
    fn moxfield_user_agent_that_is_not_a_valid_header_fails() {
        let env = EnvConfig {
            guild_id: 1,
            moxfield_user_agent: "bad\nagent".to_string(),
        };
        let err = build_provider_clients(&env).err().expect("should fail");
        assert!(matches!(
            err,
            ConfigError::InvalidEnv {
                name: "MOXFIELD_USER_AGENT",
                ..
            }
        ));
    }

    #[test]
    fn validate_accepts_same_id_on_different_providers() {
        let cfg = mtg("
collections:
  - {provider: archidekt, discord_user: a, provider_collection: '1'}
  - {provider: moxfield, discord_user: b, provider_collection: '1'}
community_decks:
  - {provider: moxfield, discord_user: a, provider_deck: d1}
");
        validate_mtg_config(&cfg).unwrap();
    }

    #[test]
    fn validate_rejects_empty_collection_id() {
        let cfg = mtg("
collections:
  - {provider: archidekt, discord_user: a, provider_collection: ''}
community_decks: []
");
        assert!(matches!(
            validate_mtg_config(&cfg),
            Err(ConfigError::Invalid(_))
        ));
    }

    #[test]
    fn validate_rejects_padded_deck_id() {
        let cfg = mtg("
collections: []
community_decks:
  - {provider: moxfield, discord_user: a, provider_deck: ' d1 '}
");
        assert!(matches!(
            validate_mtg_config(&cfg),
            Err(ConfigError::Invalid(_))
        ));
    }

    #[test]
    fn validate_rejects_duplicate_collection() {
        let cfg = mtg("
collections:
  - {provider: moxfield, discord_user: a, provider_collection: abc}
  - {provider: moxfield, discord_user: b, provider_collection: abc}
community_decks: []
");
        let err = validate_mtg_config(&cfg).unwrap_err();
        assert!(err.to_string().contains("abc"), "{err}");
    }

    #[test]
    fn validate_rejects_duplicate_deck() {
        let cfg = mtg("
collections: []
community_decks:
  - {provider: archidekt, discord_user: a, provider_deck: '9'}
  - {provider: archidekt, discord_user: b, provider_deck: '9'}
");
        assert!(validate_mtg_config(&cfg).is_err());
    }
}
