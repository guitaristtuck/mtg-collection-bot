//! HTTP clients for the collection/deck providers, plus the pure functions
//! that turn their JSON into [`SearchResultCard`]s and [`CommunityDeckMetadata`].
//!
//! Each client is built once at startup (see [`ProviderClients`], held in
//! `BotConfig::providers`) and takes a base URL so tests can point it at a
//! local mock server. Clients do no rate limiting of their own.

pub mod archidekt;
pub mod moxfield;

use crate::mtg::models::{CommunityDeckMetadata, SearchResultCard};
use reqwest::RequestBuilder;
use serde::de::DeserializeOwned;
use std::fmt;
use std::time::Duration;

pub use archidekt::ArchidektClient;
pub use moxfield::MoxfieldClient;

/// How long a single provider request may take before it is abandoned.
pub(crate) const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);

/// One page of collection search results from a single provider.
pub struct SearchPage {
    pub cards: Vec<SearchResultCard>,
    /// The provider has more matches than it returned (we only fetch the first page).
    pub may_be_incomplete: bool,
}

/// Every provider client, built once at startup.
pub struct ProviderClients {
    pub archidekt: ArchidektClient,
    pub moxfield: MoxfieldClient,
}

#[derive(Debug)]
pub enum ProviderError {
    /// The HTTP client itself could not be built (e.g. an invalid User-Agent).
    Client(reqwest::Error),
    /// The request could not be sent or the body could not be read.
    Request {
        what: &'static str,
        source: reqwest::Error,
    },
    /// The provider answered with a non-success status.
    Status {
        what: &'static str,
        status: reqwest::StatusCode,
    },
    /// The provider answered 2xx but the body was not the JSON we expect.
    Decode {
        what: &'static str,
        source: serde_json::Error,
    },
}

impl fmt::Display for ProviderError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ProviderError::Client(e) => write!(f, "could not build HTTP client: {e}"),
            ProviderError::Request { what, source } => {
                write!(f, "{what} request failed: {source}")
            }
            ProviderError::Status { what, status } => {
                write!(f, "{what} failed with status code {status}")
            }
            ProviderError::Decode { what, source } => {
                write!(f, "{what} returned an unexpected response: {source}")
            }
        }
    }
}

impl std::error::Error for ProviderError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ProviderError::Client(e) => Some(e),
            ProviderError::Request { source, .. } => Some(source),
            ProviderError::Decode { source, .. } => Some(source),
            ProviderError::Status { .. } => None,
        }
    }
}

/// Send `request` and decode a successful response body as JSON.
pub(crate) async fn get_json<T: DeserializeOwned>(
    request: RequestBuilder,
    what: &'static str,
) -> Result<T, ProviderError> {
    let resp = request
        .send()
        .await
        .map_err(|source| ProviderError::Request { what, source })?;

    let status = resp.status();
    if !status.is_success() {
        return Err(ProviderError::Status { what, status });
    }

    let body = resp
        .text()
        .await
        .map_err(|source| ProviderError::Request { what, source })?;
    serde_json::from_str(&body).map_err(|source| ProviderError::Decode { what, source })
}

/// Pick the Card Kingdom price to show: regular, then foil, then etched.
/// `None` and non-positive prices both count as missing.
pub fn select_ck_price(ck: Option<f64>, foil: Option<f64>, etched: Option<f64>) -> Option<f64> {
    [ck, foil, etched]
        .into_iter()
        .flatten()
        .find(|p| p.is_finite() && *p > 0.0)
}

/// Scryfall image URL used as a deck thumbnail.
pub(crate) fn scryfall_image_url(set: &str, cn: &str) -> String {
    format!("https://api.scryfall.com/cards/{set}/{cn}?format=image")
}

/// Strip trailing slashes so `format!("{base}/path")` never doubles them.
pub(crate) fn normalize_base_url(base_url: impl Into<String>) -> String {
    base_url.into().trim_end_matches('/').to_string()
}

/// Fallback for providers whose response does not say whether more pages exist.
pub(crate) fn is_full_page(returned: usize, page_size: usize) -> bool {
    returned >= page_size
}

/// Shared by both providers; kept here so the deck shape is built in one place.
pub(crate) fn deck_metadata(
    title: String,
    url: String,
    thumbnail_set_cn: Option<(&str, &str)>,
    original_owner: String,
    last_updated_at: String,
) -> CommunityDeckMetadata {
    CommunityDeckMetadata {
        title,
        url,
        thumbnail: thumbnail_set_cn.map(|(set, cn)| scryfall_image_url(set, cn)),
        original_owner,
        last_updated_at,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ck_price_preferred_over_foil_and_etched() {
        assert_eq!(select_ck_price(Some(1.5), Some(3.0), Some(4.0)), Some(1.5));
    }

    #[test]
    fn foil_used_when_regular_missing() {
        assert_eq!(select_ck_price(None, Some(3.0), Some(4.0)), Some(3.0));
    }

    #[test]
    fn foil_used_when_regular_is_zero() {
        assert_eq!(select_ck_price(Some(0.0), Some(3.0), None), Some(3.0));
    }

    #[test]
    fn etched_used_when_regular_and_foil_missing_or_zero() {
        assert_eq!(select_ck_price(None, Some(0.0), Some(4.0)), Some(4.0));
        assert_eq!(select_ck_price(Some(0.0), None, Some(4.0)), Some(4.0));
    }

    #[test]
    fn no_price_when_all_missing_or_zero() {
        assert_eq!(select_ck_price(None, None, None), None);
        assert_eq!(select_ck_price(Some(0.0), Some(0.0), Some(0.0)), None);
    }

    #[test]
    fn negative_and_nan_prices_are_missing() {
        assert_eq!(
            select_ck_price(Some(-1.0), Some(f64::NAN), Some(2.0)),
            Some(2.0)
        );
    }

    #[test]
    fn base_url_trailing_slash_is_trimmed() {
        assert_eq!(normalize_base_url("http://x/api/"), "http://x/api");
        assert_eq!(normalize_base_url("http://x/api"), "http://x/api");
    }

    #[test]
    fn status_error_text() {
        let e = ProviderError::Status {
            what: "Archidekt collection search",
            status: reqwest::StatusCode::NOT_FOUND,
        };
        assert_eq!(
            e.to_string(),
            "Archidekt collection search failed with status code 404 Not Found"
        );
    }
}
