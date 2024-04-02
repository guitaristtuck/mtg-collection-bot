use crate::models::config::BotConfig;
use crate::models::config::MTGCollectionProvider;
use crate::mtg::models::DISCORD_MAX_MESSAGE_LEN;
use prettytable::{Table, Row, Cell, row};

use super::models::SearchResultCard;
extern crate prettytable;

pub fn aggregate_search_results(search_results: Vec<SearchResultCard>) -> Vec<SearchResultCard> {
    let mut temp_map = std::collections::HashMap::new();
    let owner = if search_results.len() > 0 { search_results.get(0).unwrap().owner.clone() } else { String::from("unknown") };

    for item in search_results {
        *temp_map.entry(item.name.clone()).or_insert(0) += item.quantity;
    }

    let aggregated_results: Vec<SearchResultCard> = temp_map.into_iter().map(|(name, quantity)| SearchResultCard { name, quantity, owner: owner.clone() }).collect();
    
    return aggregated_results;
}

pub async fn search_collections(search_term: String,config: &BotConfig) -> String {
    println!("Searching all known collections for search term {}",search_term);

    let mut error_list: Vec<String> = Vec::new();
    let mut result_table = Table::new();
    result_table.set_format(*prettytable::format::consts::FORMAT_BOX_CHARS);
    result_table.add_row(row!["Card","Owner","Quantity"]);

    for collection in &config.mtg.collections {
        let search_response = match collection.provider {
            MTGCollectionProvider::Archidekt => crate::mtg::providers::archidekt::search(&collection.discord_user, &collection.provider_collection,&search_term).await,
            MTGCollectionProvider::Moxfield => crate::mtg::providers::moxfield::search(&collection.discord_user, &collection.provider_collection,&search_term).await,
            _ => Err(format!("Unknown provider type {}",collection.provider).into())
        };

        match search_response {
            Ok(value) => {
                let aggregated_response = aggregate_search_results(value);

                for result in aggregated_response {
                    result_table.add_row(Row::new(vec![
                        Cell::new(&result.name),
                        Cell::new(&result.owner),
                        Cell::new(&result.quantity.to_string())]));
                }
            }
            Err(e) => {
                error_list.push(format!("Could not search collection for user {}: {}",collection.discord_user,e))
            }
        }
    }

    let mut result_str = String::new();

    // print out the results table or a "no matches" message
    if result_table.len() > 1 {
        result_str.push_str(&format!("Found `{}` matches in `{}` searched collection(s) for card name `{}`:\n",(result_table.len() - 1),config.mtg.collections.len(),search_term));
        result_str.push_str(&format!("```\n{}\n```",result_table.to_string()));
    } else {
        result_str.push_str(&format!("No matches found in `{}` searched collection(s) for card name `{}`", config.mtg.collections.len(), search_term));
    }

    // print out  any errors
    if error_list.len() > 0 {
        result_str.push_str("\n");
        for error in error_list {
            result_str.push_str(&format!("{}\n",error));
        }
    }

    // Take care of too big of a message
    if result_str.len() > DISCORD_MAX_MESSAGE_LEN {
        result_str = String::from(&format!("Response too big for card name `{search_term}`. Try searching for something more unique, idiot."));
    }

    return result_str;
}