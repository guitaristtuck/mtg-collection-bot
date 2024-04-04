pub const CARD_NAME_MAX_LEN: u16 = 128;
pub const EMBED_DESCRIPTION_MAX_LEN: u16 = 4096;

// collection search result model
pub struct SearchResultCard {
    pub name: String,
    pub set: String,
    pub cn: String,
    pub quantity: i64,
    pub owner: String,
}

// embed model
pub struct SearchResultEmbed {
    pub title: String,
    pub name: String,
    pub set: String,
    pub cn: String,
    pub owners: Vec<String>,
    pub quantities: Vec<String>,
}