use reqwest::{StatusCode,Client};
use reqwest::header::CONTENT_TYPE;
use std::error::Error;
use serde::Deserialize;
use crate::mtg::models::SearchResultCard;

// Search API response structs
#[derive(Deserialize)]
struct MoxfieldCardPrices {
    ck: Option<f32>,
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

pub async fn search(discord_user: &String, collection_id: &String, search_term: &String) -> Result<Vec<SearchResultCard>, Box<dyn Error>> {
    let client = Client::new();
    let modified_search_term = format!("\"{}\"",search_term);

    log::info!("Searching moxfield collection of '{}' with collection id '{}' for term '{}'",discord_user,collection_id,search_term);
    let resp = client
        .get(format!("https://api2.moxfield.com/v1/trade-binders/{}/search?q={}", collection_id, &modified_search_term))
        .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
        .send()
        .await?;

    let moxfield_response = match resp.status() {
            StatusCode::OK => {
                let search_response: MoxfieldSearchResponse = resp.json::<MoxfieldSearchResponse>().await?;
                Ok::<MoxfieldSearchResponse, Box<dyn Error>>(search_response)
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
            ck_price: format!("{:.2}", result.card.prices.ck.unwrap_or(0.00)),
        })
    }

    return Ok(result_cards)
}