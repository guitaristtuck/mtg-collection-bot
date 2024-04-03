use crate::models::config::BotConfig;
use crate::models::config::MTGCollectionProvider;
use crate::mtg::models::DISCORD_EMBED_FIELD_MAX_LEN;
use log;

use super::models::SearchResultCard;

use serenity::builder::{CreateEmbed,CreateInteractionResponse,CreateInteractionResponseMessage,CreateEmbedFooter};

pub fn aggregate_search_results(search_results: Vec<SearchResultCard>) -> Vec<SearchResultCard> {
    let mut temp_map = std::collections::HashMap::new();
    let owner = if search_results.len() > 0 { search_results.get(0).unwrap().owner.clone() } else { String::from("unknown") };

    for item in search_results {
        *temp_map.entry(item.name.clone()).or_insert(0) += item.quantity;
    }

    let aggregated_results: Vec<SearchResultCard> = temp_map.into_iter().map(|(name, quantity)| SearchResultCard { name, quantity, owner: owner.clone() }).collect();
    
    return aggregated_results;
}

pub async fn search_collections(search_term: String,config: &BotConfig) -> CreateInteractionResponse {
    log::info!("Searching all known collections for search term {}",search_term);

    let mut errors: String = String::new();
    let mut table: Vec<String> = vec![
        String::new(),
        String::new(),
        String::new()
    ];
    let mut result_count = 0;


    for collection in &config.mtg.collections {
        let search_response = match collection.provider {
            MTGCollectionProvider::Archidekt => crate::mtg::providers::archidekt::search(&collection.discord_user, &collection.provider_collection,&search_term).await,
            MTGCollectionProvider::Moxfield => crate::mtg::providers::moxfield::search(&collection.discord_user, &collection.provider_collection,&search_term).await,
        };

        match search_response {
            Ok(value) => {
                let aggregated_response = aggregate_search_results(value);

                for result in aggregated_response {
                    result_count += 1;
                    table[0] += &format!("{}\n",result.name);
                    table[1] += &format!("{}\n",result.owner);
                    table[2] += &format!("{}\n",result.quantity);
                }
            }
            Err(e) => {
                errors.push_str(&format!("Could not search collection for user {}: {}\n",collection.discord_user,e))
            }
        }
    }

    // return an error if things were too long
    for str in &table {
        if str.len() > DISCORD_EMBED_FIELD_MAX_LEN {
            return CreateInteractionResponse::Message(CreateInteractionResponseMessage::new().content("Search returned too many results to display"))
        }
    }

    let embed: CreateEmbed;

    // print out the results table or a "no matches" message
    if result_count > 0 {
        embed = CreateEmbed::new()
            .title("Search Results")
            .description(&format!("Found `{}` matches in `{}` searched collection(s) for card name `{}`:\n",result_count,config.mtg.collections.len(),search_term))
            .field("Card",table[0].clone(), true)
            .field("Owner", table[1].clone(), true)
            .field("Quantity", table[2].clone(), true)
            .footer(CreateEmbedFooter::new(errors));
    } else {
        embed = CreateEmbed::new()
            .title("Search Results")
            .description(&format!("No matches found in `{}` searched collection(s) for card name `{}`", config.mtg.collections.len(), search_term))
            .footer(CreateEmbedFooter::new(errors));
    }

    CreateInteractionResponse::Message(CreateInteractionResponseMessage::new().add_embed(embed))
}