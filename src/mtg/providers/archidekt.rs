use reqwest::{StatusCode,Client};
use reqwest::header::CONTENT_TYPE;
use std::error::Error;
use serde::Deserialize;
use crate::mtg::models::SearchResultCard;

// Search API response structs
#[derive(Deserialize)]
struct ArchidektCardPrices {
    ck: Option<f32>,
    ck_foil: Option<f32>,
    ck_etched: Option<f32>,
}

#[derive(Deserialize)]
struct ArchidektCardVariantBEdition {
    editioncode: String,
}
    
#[derive(Deserialize)]
#[serde(untagged)]
enum ArchidektCard {
    ArchidektCardVariantA {
        name: String,
        set: String,
        cn: String,
        prices: ArchidektCardPrices,
    },
    ArchidektCardVariantB {
        name: String,
        edition: ArchidektCardVariantBEdition,
        #[serde(rename = "collectorNumber")]
        collector_number: String,
        prices: ArchidektCardPrices,
    },
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

pub async fn search(discord_user: String, collection_id: String, search_term: String) -> Result<Vec<SearchResultCard>, Box<dyn Error + Send + Sync>> {
    let client = Client::new();

    log::info!("Searching archidekt collection of '{}' with collection id '{}' for term '{}'",discord_user,collection_id,search_term);
    let resp = client
        .get(format!("https://www.archidekt.com/api/collection/{}/?cardName={}", collection_id, urlencoding::encode(&search_term)))
        .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
        .send()
        .await?;

    let archidekt_response = match resp.status() {
            StatusCode::OK => {
                let search_response: ArchidektSearchResponse = resp.json::<ArchidektSearchResponse>().await?;
                Ok::<ArchidektSearchResponse, Box<dyn Error + Send + Sync>>(search_response)
            }
            status => Err(format!("Archidekt collection search failed with status code {}",status).into()),
        }?;

    let mut result_cards: Vec<SearchResultCard> = Vec::new();

    for result in archidekt_response.results {
        let card = result.card;
        match card {
            
            ArchidektCard::ArchidektCardVariantA { name, set, cn, prices } => {
                result_cards.push(SearchResultCard {
                    name: name,
                    set: set,
                    cn: cn,
                    quantity: result.quantity,
                    owner: discord_user.clone(),
                    ck_price: format!("{:.2}", prices.ck.unwrap_or(
                        prices.ck_foil.unwrap_or(
                            prices.ck_etched.unwrap_or(0.00)
                        )
                    )),
                });
                
            }
            ArchidektCard::ArchidektCardVariantB { name, edition, collector_number, prices } => {
                result_cards.push(SearchResultCard {
                    name: name,
                    set: edition.editioncode,
                    cn: collector_number,
                    quantity: result.quantity,
                    owner: discord_user.clone(),
                    ck_price: format!("{:.2}", prices.ck.unwrap_or(
                        prices.ck_foil.unwrap_or(
                            prices.ck_etched.unwrap_or(0.00)
                        )
                    )),
                });
            }
        }
        
    }

    return Ok(result_cards)
}