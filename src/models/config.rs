use serde::Deserialize;

#[derive(Deserialize)]
pub enum MTGCollectionProvider {
    Archidekt,
}

#[derive(Deserialize)]
pub struct MTGCollectionConfig {
    pub provider: MTGCollectionProvider,
    pub discord_user: String,
    pub provider_collection: String,
}

#[derive(Deserialize)]
pub struct MTGConfig {
    pub collections: Vec<MTGCollectionConfig>,
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