use reqwest::{StatusCode,Client};
use reqwest::header::CONTENT_TYPE;
use std::error::Error;
use serde::Deserialize;
use crate::mtg::models::SearchResultCard;

// Search API response structs
#[derive(Deserialize)]
struct ArchidektCard {
    name: String,
    set: String,
    cn: String,
}
    

#[derive(Deserialize)]
struct ArchidektSearchResult {
    card: ArchidektCard,
    quantity: i64,
}

#[derive(Deserialize)]
struct ArchidektSearchResponse {
    results: Vec<ArchidektSearchResult>
}

pub async fn search(discord_user: &String, collection_id: &String, search_term: &String) -> Result<Vec<SearchResultCard>, Box<dyn Error>> {
    let client = Client::new();

    log::info!("Searching archidekt collection of '{}' with collection id '{}' for term '{}'",discord_user,collection_id,search_term);
    let resp = client
        .get(format!("https://www.archidekt.com/api/collection/{}/?cardName={}", collection_id, urlencoding::encode(search_term)))
        .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
        .send()
        .await?;

    let archidekt_response = match resp.status() {
            StatusCode::OK => {
                let search_response: ArchidektSearchResponse = resp.json::<ArchidektSearchResponse>().await?;
                Ok::<ArchidektSearchResponse, Box<dyn Error>>(search_response)
            }
            status => Err(format!("Archidekt collection search failed with status code {}",status).into()),
        }?;

    let mut result_cards: Vec<SearchResultCard> = Vec::new();

    for result in archidekt_response.results {
        result_cards.push(SearchResultCard {
            name: result.card.name,
            set: result.card.set,
            cn: result.card.cn,
            quantity: result.quantity,
            owner: discord_user.clone(),
        })
    }

    return Ok(result_cards)
}