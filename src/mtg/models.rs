pub const CARD_NAME_MAX_LEN: u16 = 128;
pub const DISCORD_MAX_MESSAGE_LEN: usize = 2000;

// collection search result model
pub struct SearchResultCard {
    pub name: String,
    pub quantity: i64,
    pub owner: String,
}