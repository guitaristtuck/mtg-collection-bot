use reqwest::{Client, StatusCode};
use std::error::Error;
use serde::Deserialize;
use crate::mtg::models::{SearchResultCard, CommunityDeckMetadata};

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
        .get(format!("https://archidekt.com/api/collection/{}/", collection_id))
        .query(&[("cardName", search_term)])
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
            
            ArchidektCard::ArchidektCardVariantA { name, set, cn, prices, .. } => {
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
            ArchidektCard::ArchidektCardVariantB { name, edition, collector_number, prices , ..} => {
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
    
#[derive(Deserialize)]
#[serde(untagged)]
enum ArchidektDeckCardDetails {
    ArchidektDeckCardDetailsA {
        set: String,
        cn: String,
    },
    ArchidektDeckCardDetailsB {
        edition: ArchidektCardVariantBEdition,
        #[serde(rename = "collectorNumber")]
        collector_number: String,
    },
}

#[derive(Deserialize)]
struct ArchidektDeckCard {
    card: ArchidektDeckCardDetails,
    categories: Vec<String>,
}

#[derive(Deserialize)]
struct Owner {
    username: String,
}

#[derive(Deserialize)]
struct ArchidektDeck {
    name: String,
    cards: Vec<ArchidektDeckCard>,
    owner: Owner,
    #[serde(rename = "updatedAt")]
    updated_at: String,
}

pub async fn get_deck(discord_user: String, deck_id: String) -> Result<CommunityDeckMetadata, Box<dyn Error + Send + Sync>> {
    let client = Client::new();

    log::info!("Fetching archidekt deck metadata owned by  '{}' for deck id '{}'",discord_user,deck_id);
    let resp = client
        .get(format!("https://www.archidekt.com/api/decks/{}/", deck_id))
        .send()
        .await?;

    let archidekt_response = match resp.status() {
            StatusCode::OK => {
                let deserialized: serde_json::Result<ArchidektDeck> = serde_json::from_str(&resp.text().await?);

                let deck_response =  match deserialized {
                    Ok(data) => data,
                    Err(e) => {
                        log::error!("Failed to deserialize: {}", e);
                        // Optionally, you can output the response again here for clarity
                        return Err(Box::new(e))
                    }
                };
                //let deck_response: ArchidektDeck = resp.json::<ArchidektDeck>().await?;
                Ok::<ArchidektDeck, Box<dyn Error + Send + Sync>>(deck_response)
            }
            status => Err(format!("archidekt deck lookup failed with status code {}",status).into()),
        }?;

    // get any cards with the commander category from the api response
    let commanders: Vec<ArchidektDeckCard> = archidekt_response.cards.into_iter()
        .filter(|card| card.categories.clone().contains(&"Commander".to_string()))
        .collect();

    // consider only the first commander card for the thumbnail. extract those values
    let set_cn_tuple: (String, String) = match &commanders.get(0).unwrap().card {
        ArchidektDeckCardDetails::ArchidektDeckCardDetailsA { set, cn, .. } => {
            (set.into(), cn.into())
        }
        ArchidektDeckCardDetails::ArchidektDeckCardDetailsB { edition, collector_number, .. } => {
            (edition.editioncode.clone(), collector_number.into())
        }
    };

    return Ok(CommunityDeckMetadata{
        title: archidekt_response.name,
        url: format!("https://archidekt.com/decks/{}",deck_id),
        thumbnail: format!("https://api.scryfall.com/cards/{}/{}?format=image",set_cn_tuple.0,set_cn_tuple.1),
        original_owner: archidekt_response.owner.username,
        last_updated_at: archidekt_response.updated_at,
    })
}