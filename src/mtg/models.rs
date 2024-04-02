pub const CARD_NAME_MAX_LEN: u16 = 128;
pub const DISCORD_EMBED_FIELD_MAX_LEN: usize = 1024;

// collection search result model
pub struct SearchResultCard {
    pub name: String,
    pub quantity: i64,
    pub owner: String,
}