pub enum MTGCollectionProvider {
    Archidekt,
}

pub enum MTGCollectionProviderData {
    Archidekt(ArkidektProviderData)
}

pub struct JWT {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: i64,
}

pub struct ArkidektProviderData {
    pub username: String,
    pub user_id: i64,
    pub jwt: JWT,
}

pub struct MTGCollectionConfig {
    pub provider: MTGCollectionProvider,
    pub discord_user: String,
    pub provider_user: String,
    pub provider_data: MTGCollectionProviderData,
}

pub struct MTGConfig {
    pub collections: Vec<MTGCollectionConfig>,
}

pub struct BotConfig {
    pub mtg: MTGConfig,
}