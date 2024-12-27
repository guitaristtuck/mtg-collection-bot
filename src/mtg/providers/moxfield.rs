use reqwest::{StatusCode,Client};
use reqwest::header::CONTENT_TYPE;
use std::error::Error;
use serde::Deserialize;
use crate::mtg::models::{SearchResultCard,CommunityDeckMetadata};

// Search API response structs
#[derive(Deserialize)]
struct MoxfieldCardPrices {
    ck: Option<f32>,
    ck_foil: Option<f32>,
    ck_etched: Option<f32>,
}

#[derive(Deserialize)]
struct MoxfieldCard {
    name: String,
    set: String,
    cn: String,
    prices: MoxfieldCardPrices,
}

#[derive(Deserialize)]
struct MoxfieldSearchResult {
    card: MoxfieldCard,
    quantity: i64,
}

#[derive(Deserialize)]
struct MoxfieldSearchResponse {
    data: Vec<MoxfieldSearchResult>
}

pub async fn search(discord_user: String, collection_id: String, search_term: String) -> Result<Vec<SearchResultCard>, Box<dyn Error + Send + Sync>> {
    let client = Client::new();
    let modified_search_term = format!("\"{}\"",search_term);

    log::info!("Searching moxfield collection of '{}' with collection id '{}' for term '{}'",discord_user,collection_id,search_term);
    let resp = client
        .get(format!("https://api2.moxfield.com/v1/trade-binders/{}/search?q={}", collection_id, &modified_search_term))
        .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
        // Added this header to circumvent cloudflare's bot detection
        .header("User-Agent", "Mozilla/5.0 (Macintosh; Intel Mac OS X 10.15; rv:133.0) Gecko/20100101 Firefox/133.0")
        .send()
        .await?;

    let moxfield_response = match resp.status() {
            StatusCode::OK => {
                let search_response: MoxfieldSearchResponse = resp.json::<MoxfieldSearchResponse>().await?;
                Ok::<MoxfieldSearchResponse, Box<dyn Error + Send + Sync>>(search_response)
            }
            status => Err(format!("Moxfield collection search failed with status code {}",status).into()),
        }?;

    let mut result_cards: Vec<SearchResultCard> = Vec::new();

    for result in moxfield_response.data {
        result_cards.push(SearchResultCard {
            name: result.card.name,
            set: result.card.set,
            cn: result.card.cn,
            quantity: result.quantity,
            owner: discord_user.clone(),
            ck_price: format!("{:.2}", result.card.prices.ck.unwrap_or(
                result.card.prices.ck_foil.unwrap_or(
                    result.card.prices.ck_etched.unwrap_or(0.00)
                )
            )),
        })
    }

    return Ok(result_cards)
}

#[derive(Deserialize)]
struct CreatedByUser {
    #[serde(rename = "displayName")]
    display_name: String,
}

#[derive(Deserialize)]
struct MoxfieldDeck {
    name: String,
    main: MoxfieldCard,
    #[serde(rename = "publicUrl")]
    public_url: String,
    #[serde(rename = "createdByUser")]
    created_by_user: CreatedByUser,
    #[serde(rename = "lastUpdatedAtUtc")]
    last_updated_at_utc: String,
}

pub async fn get_deck(discord_user: String, deck_id: String) -> Result<CommunityDeckMetadata, Box<dyn Error + Send + Sync>> {
    let client = Client::new();

    log::info!("Fetching moxfield deck metadata owned by  '{}' for deck id '{}'",discord_user,deck_id);
    let resp = client
        .get(format!("https://api2.moxfield.com/v3/decks/all/{}", deck_id))
        .send()
        .await?;

    let moxfield_response = match resp.status() {
            StatusCode::OK => {
                let deck_response: MoxfieldDeck = resp.json::<MoxfieldDeck>().await?;
                Ok::<MoxfieldDeck, Box<dyn Error + Send + Sync>>(deck_response)
            }
            status => Err(format!("Moxfield deck lookup failed with status code {}",status).into()),
        }?;

    return Ok(CommunityDeckMetadata{
        title: moxfield_response.name,
        url: moxfield_response.public_url,
        thumbnail: format!("https://api.scryfall.com/cards/{}/{}?format=image",moxfield_response.main.set,moxfield_response.main.cn),
        original_owner: moxfield_response.created_by_user.display_name,
        last_updated_at: moxfield_response.last_updated_at_utc,
    })
}
