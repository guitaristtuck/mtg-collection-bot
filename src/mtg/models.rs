pub const CARD_NAME_MAX_LEN: u16 = 128;
pub const EMBED_DESCRIPTION_MAX_LEN: u16 = 4096;

// collection search result model
pub struct SearchResultCard {
    pub name: String,
    pub set: String,
    pub cn: String,
    pub quantity: i64,
    pub owner: String,
    /// Card Kingdom price chosen by `providers::select_ck_price`; `None` when unknown.
    pub ck_price: Option<f64>,
}

// collection search embed model
pub struct SearchResultEmbed {
    pub title: String,
    pub name: String,
    pub set: String,
    pub cn: String,
    pub owners: Vec<String>,
    pub quantities: Vec<String>,
    pub ck_price: Option<f64>,
}

// community deck list model
pub struct CommunityDeckMetadata {
    pub title: String,
    pub url: String,
    /// `None` when the deck has no card to use as a thumbnail (e.g. no commander).
    pub thumbnail: Option<String>,
    pub original_owner: String,
    pub last_updated_at: String,
}
