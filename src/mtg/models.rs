pub const CARD_NAME_MAX_LEN: u16 = 128;

// collection search result model
pub struct SearchResultCard {
    name: String,
    quantity: i64,
}

pub struct SearchResult {
    cards: Vec<SearchResultCard>,
}