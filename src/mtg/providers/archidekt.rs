use super::{
    deck_metadata, get_json, is_full_page, normalize_base_url, select_ck_price, ProviderError,
    SearchPage, REQUEST_TIMEOUT,
};
use crate::mtg::models::{CommunityDeckMetadata, SearchResultCard};
use reqwest::Client;
use serde::Deserialize;

pub const ARCHIDEKT_API_BASE_URL: &str = "https://archidekt.com/api";
/// Collection search returns 25 results per page; a `pageSize` parameter is ignored.
pub const ARCHIDEKT_SEARCH_PAGE_SIZE: usize = 25;

const SEARCH: &str = "Archidekt collection search";
const DECK: &str = "Archidekt deck lookup";

pub struct ArchidektClient {
    http: Client,
    base_url: String,
}

impl ArchidektClient {
    /// Client for the real Archidekt API.
    pub fn new() -> Result<Self, ProviderError> {
        Self::with_base_url(ARCHIDEKT_API_BASE_URL)
    }

    /// Client for an Archidekt-compatible API at `base_url` (e.g. a mock server).
    pub fn with_base_url(base_url: impl Into<String>) -> Result<Self, ProviderError> {
        let http = Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .build()
            .map_err(ProviderError::Client)?;
        Ok(Self {
            http,
            base_url: normalize_base_url(base_url),
        })
    }

    /// Search one collection for `search_term`; cards are attributed to `owner`.
    pub async fn search(
        &self,
        collection_id: &str,
        search_term: &str,
        owner: &str,
    ) -> Result<SearchPage, ProviderError> {
        log::info!("Searching archidekt collection '{collection_id}' for term '{search_term}'");
        let request = self
            .http
            .get(format!("{}/collection/{collection_id}/", self.base_url))
            .query(&[("cardName", search_term)]);
        let response: SearchResponse = get_json(request, SEARCH).await?;
        Ok(search_page(response, owner))
    }

    pub async fn get_deck(&self, deck_id: &str) -> Result<CommunityDeckMetadata, ProviderError> {
        log::info!("Fetching archidekt deck metadata for deck id '{deck_id}'");
        let request = self.http.get(format!("{}/decks/{deck_id}/", self.base_url));
        let deck: Deck = get_json(request, DECK).await?;
        Ok(deck_from_response(deck, deck_id))
    }
}

// ---- search response ----

#[derive(Deserialize, Default)]
struct Prices {
    ck: Option<f64>,
    // Archidekt spells this `ckfoil`; the alias keeps the older spelling working.
    #[serde(alias = "ck_foil")]
    ckfoil: Option<f64>,
    // Not currently sent by Archidekt.
    #[serde(alias = "ck_etched")]
    cketched: Option<f64>,
}

impl Prices {
    fn ck_price(&self) -> Option<f64> {
        select_ck_price(self.ck, self.ckfoil, self.cketched)
    }
}

#[derive(Deserialize)]
struct Edition {
    editioncode: String,
}

/// Archidekt has returned cards in two shapes; accept either.
#[derive(Deserialize)]
#[serde(untagged)]
enum Card {
    /// Current shape: `edition.editioncode` + `collectorNumber`.
    Edition {
        name: String,
        edition: Edition,
        #[serde(rename = "collectorNumber")]
        collector_number: String,
        #[serde(default)]
        prices: Prices,
    },
    /// Flat shape: `set` + `cn`.
    Flat {
        name: String,
        set: String,
        cn: String,
        #[serde(default)]
        prices: Prices,
    },
}

#[derive(Deserialize)]
struct SearchResult {
    card: Card,
    quantity: i64,
}

#[derive(Deserialize)]
struct SearchResponse {
    results: Vec<SearchResult>,
    count: Option<u64>,
    next: Option<String>,
}

/// Parse an Archidekt collection search response body.
pub fn parse_search(json: &str, owner: &str) -> Result<SearchPage, serde_json::Error> {
    serde_json::from_str(json).map(|r| search_page(r, owner))
}

fn search_page(response: SearchResponse, owner: &str) -> SearchPage {
    let returned = response.results.len();
    let may_be_incomplete = response.next.is_some()
        || match response.count {
            Some(count) => count > returned as u64,
            None => is_full_page(returned, ARCHIDEKT_SEARCH_PAGE_SIZE),
        };

    let cards = response
        .results
        .into_iter()
        .map(|result| {
            let (name, set, cn, prices) = match result.card {
                Card::Edition {
                    name,
                    edition,
                    collector_number,
                    prices,
                } => (name, edition.editioncode, collector_number, prices),
                Card::Flat {
                    name,
                    set,
                    cn,
                    prices,
                } => (name, set, cn, prices),
            };
            SearchResultCard {
                name,
                set,
                cn,
                quantity: result.quantity,
                owner: owner.to_string(),
                ck_price: prices.ck_price(),
            }
        })
        .collect();

    SearchPage {
        cards,
        may_be_incomplete,
    }
}

// ---- deck response ----

#[derive(Deserialize)]
#[serde(untagged)]
enum DeckCardDetails {
    Edition {
        edition: Edition,
        #[serde(rename = "collectorNumber")]
        collector_number: String,
    },
    Flat {
        set: String,
        cn: String,
    },
}

impl DeckCardDetails {
    fn set_cn(&self) -> (&str, &str) {
        match self {
            DeckCardDetails::Edition {
                edition,
                collector_number,
            } => (&edition.editioncode, collector_number),
            DeckCardDetails::Flat { set, cn } => (set, cn),
        }
    }
}

#[derive(Deserialize)]
struct DeckCard {
    card: DeckCardDetails,
    #[serde(default)]
    categories: Vec<String>,
}

#[derive(Deserialize)]
struct Owner {
    username: String,
}

#[derive(Deserialize)]
struct Deck {
    name: String,
    #[serde(default)]
    cards: Vec<DeckCard>,
    owner: Owner,
    #[serde(rename = "updatedAt")]
    updated_at: String,
}

/// Parse an Archidekt deck response body.
pub fn parse_deck(json: &str, deck_id: &str) -> Result<CommunityDeckMetadata, serde_json::Error> {
    serde_json::from_str(json).map(|d| deck_from_response(d, deck_id))
}

fn deck_from_response(deck: Deck, deck_id: &str) -> CommunityDeckMetadata {
    // The first Commander card is the thumbnail; decks without one get none.
    let commander = deck
        .cards
        .iter()
        .find(|c| c.categories.iter().any(|cat| cat == "Commander"))
        .map(|c| c.card.set_cn());

    deck_metadata(
        deck.name,
        format!("https://archidekt.com/decks/{deck_id}"),
        commander,
        deck.owner.username,
        deck.updated_at,
    )
}
