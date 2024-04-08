use serde::Deserialize;
use std::fmt;

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

pub struct BotConfig {
    pub mtg: MTGConfig,
}

pub fn load_config() -> BotConfig {
    let f = std::fs::File::open("config/mtg.yaml").expect("Could not open file 'config/mtg.yaml'");
    let mtg_config: MTGConfig = serde_yaml::from_reader(f).expect("Could not load config 'config/mtg.yaml'");

    BotConfig {
        mtg: mtg_config,
    }
}