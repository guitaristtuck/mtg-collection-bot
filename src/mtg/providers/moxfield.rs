use super::rate_limit::RateLimiter;
use super::{
    deck_metadata, get_json, is_full_page, normalize_base_url, select_ck_price, ProviderError,
    SearchPage, REQUEST_TIMEOUT,
};
use crate::mtg::models::{CommunityDeckMetadata, SearchResultCard};
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT};
use reqwest::Client;
use serde::Deserialize;
use std::time::Duration;

pub const MOXFIELD_API_BASE_URL: &str = "https://api2.moxfield.com";
/// Trade binder search returns 25 results per page by default (`pageSize` in the response).
pub const MOXFIELD_SEARCH_PAGE_SIZE: usize = 25;

const SEARCH: &str = "Moxfield collection search";
const DECK: &str = "Moxfield deck lookup";

/// Moxfield allows at most one request per second.
pub const MOXFIELD_MIN_REQUEST_INTERVAL: Duration = Duration::from_secs(1);

/// Shared Moxfield client. Every request carries the whitelisted User-Agent
/// (Moxfield's Cloudflare rules block requests without it) and `Accept: application/json`.
///
/// Moxfield allows at most 1 request per second. Every request made through a client
/// waits on that client's [`RateLimiter`], so the single client in `ProviderClients`
/// paces collection search and community decks (and concurrent commands) together.
pub struct MoxfieldClient {
    http: Client,
    base_url: String,
    limiter: RateLimiter,
}

impl MoxfieldClient {
    /// Client for the real Moxfield API.
    pub fn new(user_agent: &str) -> Result<Self, ProviderError> {
        Self::with_base_url(MOXFIELD_API_BASE_URL, user_agent)
    }

    /// Client for a Moxfield-compatible API at `base_url` (e.g. a mock server).
    pub fn with_base_url(
        base_url: impl Into<String>,
        user_agent: &str,
    ) -> Result<Self, ProviderError> {
        let mut headers = HeaderMap::new();
        headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
        let http = Client::builder()
            .user_agent(user_agent)
            .default_headers(headers)
            .timeout(REQUEST_TIMEOUT)
            .build()
            .map_err(ProviderError::Client)?;
        Ok(Self {
            http,
            base_url: normalize_base_url(base_url),
            limiter: RateLimiter::new(MOXFIELD_MIN_REQUEST_INTERVAL),
        })
    }

    /// Search one trade binder for `search_term` (as a phrase); cards are attributed to `owner`.
    pub async fn search(
        &self,
        binder_id: &str,
        search_term: &str,
        owner: &str,
    ) -> Result<SearchPage, ProviderError> {
        log::info!("Searching moxfield binder '{binder_id}' for term '{search_term}'");
        let request = self
            .http
            .get(format!(
                "{}/v1/trade-binders/{binder_id}/search",
                self.base_url
            ))
            .query(&[("q", phrase_query(search_term))]);
        self.limiter.acquire().await;
        let response: SearchResponse = get_json(request, SEARCH).await?;
        Ok(search_page(response, owner))
    }

    pub async fn get_deck(&self, deck_id: &str) -> Result<CommunityDeckMetadata, ProviderError> {
        log::info!("Fetching moxfield deck metadata for deck id '{deck_id}'");
        let request = self
            .http
            .get(format!("{}/v3/decks/all/{deck_id}", self.base_url));
        self.limiter.acquire().await;
        let deck: Deck = get_json(request, DECK).await?;
        Ok(deck_from_response(deck))
    }
}

/// Wrap the term in quotes so Moxfield matches it as a phrase.
/// URL encoding is left to the HTTP client.
pub fn phrase_query(search_term: &str) -> String {
    format!("\"{search_term}\"")
}

// ---- search response ----

#[derive(Deserialize, Default)]
struct Prices {
    ck: Option<f64>,
    ck_foil: Option<f64>,
    ck_etched: Option<f64>,
}

#[derive(Deserialize)]
struct Card {
    name: String,
    set: String,
    cn: String,
    #[serde(default)]
    prices: Prices,
}

#[derive(Deserialize)]
struct SearchResult {
    card: Card,
    quantity: i64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SearchResponse {
    data: Vec<SearchResult>,
    total_results: Option<u64>,
}

/// Parse a Moxfield trade binder search response body.
pub fn parse_search(json: &str, owner: &str) -> Result<SearchPage, serde_json::Error> {
    serde_json::from_str(json).map(|r| search_page(r, owner))
}

fn search_page(response: SearchResponse, owner: &str) -> SearchPage {
    let returned = response.data.len();
    // `hasMore` is unreliable (seen `false` with totalPages 2), so use totalResults.
    let may_be_incomplete = match response.total_results {
        Some(total) => total > returned as u64,
        None => is_full_page(returned, MOXFIELD_SEARCH_PAGE_SIZE),
    };

    let cards = response
        .data
        .into_iter()
        .map(|result| {
            let card = result.card;
            SearchResultCard {
                ck_price: select_ck_price(
                    card.prices.ck,
                    card.prices.ck_foil,
                    card.prices.ck_etched,
                ),
                name: card.name,
                set: card.set,
                cn: card.cn,
                quantity: result.quantity,
                owner: owner.to_string(),
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
struct DeckMainCard {
    set: String,
    cn: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreatedByUser {
    display_name: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Deck {
    name: String,
    /// The deck's featured card (normally the commander); absent on some decks.
    main: Option<DeckMainCard>,
    public_url: String,
    created_by_user: CreatedByUser,
    last_updated_at_utc: String,
}

/// Parse a Moxfield deck response body.
pub fn parse_deck(json: &str) -> Result<CommunityDeckMetadata, serde_json::Error> {
    serde_json::from_str(json).map(deck_from_response)
}

fn deck_from_response(deck: Deck) -> CommunityDeckMetadata {
    deck_metadata(
        deck.name,
        deck.public_url,
        deck.main.as_ref().map(|m| (m.set.as_str(), m.cn.as_str())),
        deck.created_by_user.display_name,
        deck.last_updated_at_utc,
    )
}
