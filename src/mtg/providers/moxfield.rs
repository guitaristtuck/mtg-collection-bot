use prettytable::format;
use reqwest::{StatusCode,Client};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, AUTHORIZATION, ACCEPT, CONTENT_TYPE};
use std::error::Error;
use serde_json::json;
use serde::Deserialize;
use crate::mtg::models::{SearchResultCard};

use std::env;

// Search API response structs
// data[].quantity
// data[].card.name
#[derive(Deserialize)]
struct MoxfieldCard {
    name: String,
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

    println!("Searching library of collection id {} for term {}",collection_id,search_term);
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
            quantity: result.quantity,
            owner: discord_user.clone(),
        })
    }

    return Ok(result_cards)
}