use std::collections::HashSet;
use std::path::PathBuf;

use fb3k_discord_bot::models::config::{load_config_from, ConfigError, EnvConfig};

fn real_config_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("config")
}

fn test_env() -> EnvConfig {
    EnvConfig {
        guild_id: 1,
        moxfield_user_agent: "test-agent".to_string(),
    }
}

#[test]
fn real_config_parses() {
    let config = load_config_from(&real_config_dir(), test_env()).expect("config/ should load");
    assert!(!config.mtg.collections.is_empty());
    assert_eq!(config.env, test_env());
}

#[test]
fn real_config_ids_are_non_empty_and_trimmed() {
    let config = load_config_from(&real_config_dir(), test_env()).unwrap();
    let ids = config
        .mtg
        .collections
        .iter()
        .map(|c| &c.provider_collection)
        .chain(config.mtg.community_decks.iter().map(|d| &d.provider_deck));
    for id in ids {
        assert!(!id.is_empty(), "empty id in config/mtg.yaml");
        assert_eq!(id.trim(), id, "id {id:?} has surrounding whitespace");
    }
}

#[test]
fn real_config_has_no_duplicate_provider_ids() {
    let config = load_config_from(&real_config_dir(), test_env()).unwrap();

    let mut seen = HashSet::new();
    for c in &config.mtg.collections {
        assert!(
            seen.insert((c.provider, c.provider_collection.as_str())),
            "duplicate collection ({}, {})",
            c.provider,
            c.provider_collection
        );
    }

    let mut seen = HashSet::new();
    for d in &config.mtg.community_decks {
        assert!(
            seen.insert((d.provider, d.provider_deck.as_str())),
            "duplicate community deck ({}, {})",
            d.provider,
            d.provider_deck
        );
    }
}

#[test]
fn missing_config_dir_is_an_error_naming_the_file() {
    let dir = real_config_dir().join("does-not-exist");
    let err = load_config_from(&dir, test_env())
        .err()
        .expect("should fail");
    assert!(matches!(err, ConfigError::Io { .. }));
    assert!(err.to_string().contains("mtg.yaml"), "{err}");
}
