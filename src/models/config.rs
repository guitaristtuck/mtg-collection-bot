use serde::Deserialize;
use std::fmt;
use std::sync::Mutex;
use chrono::{Utc,Duration};
use crate::interactions::nubby::NUBBY_ANNOUNCE_RESET_HOURS;

#[derive(Deserialize)]
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
    pub nubby_last_announced: Mutex<chrono::DateTime<Utc>>
}


fn default_last_announced() -> Mutex<chrono::DateTime<Utc>> {
    let nubby_last_announced: Mutex<chrono::DateTime<Utc>>;
    nubby_last_announced = Mutex::new(Utc::now() - Duration::hours(NUBBY_ANNOUNCE_RESET_HOURS + 1));
    return nubby_last_announced;
}

#[derive(Deserialize)]
pub struct CommonConfig {
    pub general_channel_id: u64,
}

pub struct BotConfig {
    pub mtg: MTGConfig,
    pub nubby: NubbyConfig,
    pub common: CommonConfig,
}

pub fn load_config() -> BotConfig {
    let f = std::fs::File::open("config/mtg.yaml").expect("Could not open file 'config/mtg.yaml'");
    let mtg_config: MTGConfig = serde_yaml::from_reader(f).expect("Could not load config 'config/mtg.yaml'");

    let f = std::fs::File::open("config/common.yaml").expect("Could not open file 'config/common.yaml'");
    let common_config: CommonConfig = serde_yaml::from_reader(f).expect("Could not load config 'config/common.yaml'");

    let f = std::fs::File::open("config/nubby.yaml").expect("Could not open file 'config/nubby.yaml'");
    let nubby_config: NubbyConfig = serde_yaml::from_reader(f).expect("Could not load config 'config/nubby.yaml'");

    BotConfig {
        mtg: mtg_config,
        common: common_config,
        nubby: nubby_config,
    }
}