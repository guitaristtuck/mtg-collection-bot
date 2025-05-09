use crate::models::config::BotConfig;
use crate::models::config::MTGCollectionProvider;
use log;

use std::env;
use super::models::{SearchResultCard,SearchResultEmbed,EMBED_DESCRIPTION_MAX_LEN};
use serenity::constants::EMBED_MAX_COUNT;
use tokio::time::{Duration, interval};

use serenity::builder::{CreateEmbed,EditInteractionResponse};

fn generate_embed_data_from_search_results(search_results: Vec<SearchResultCard>) -> Vec<SearchResultEmbed> {
    let mut temp_map = std::collections::HashMap::new();

    // use a hashmap to aggregate SearchResultCards together for a given name / set name and card number. This results in a
    // vec containing all result matches across all users for the given name / set / cn
    for item in search_results {
        let embed_title = format!("{} [{}:{}]",&item.name, &item.set.to_uppercase(), &item.cn);
        temp_map.entry(embed_title).or_insert(Vec::new()).push(item);
    }

    let results: Vec<SearchResultEmbed> = temp_map
        .into_iter()
        .map(|(title, cards)| {
            // roll up any duplicates per owner by squashing together and summing quantities
            let mut quantities_by_owner: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
            for card in &cards {
                *quantities_by_owner.entry(card.owner.clone()).or_insert(0) += card.quantity;
            }

            // Extracting owners and quantities from the SearchResultCard Vec
            let owners: Vec<String> = quantities_by_owner.keys().cloned().collect();
            let quantities: Vec<String> = quantities_by_owner.values().map(|q| q.to_string()).collect();

            // push a result embed struct to make it easier to build the resulting message
            SearchResultEmbed {
                title,
                name: cards[0].name.clone(),
                set: cards[0].set.clone(),
                cn: cards[0].cn.clone(),
                owners,
                quantities,
                ck_price: cards[0].ck_price.clone()
            }
        })
        .collect();

    return results;
}

fn generate_scryfall_page_link(title: &String, card_name: &String, set: &String, cn: &String) -> String {
    return format!("[{title}](https://scryfall.com/card/{set}/{cn}/{})", card_name.to_lowercase().replace(" ","-"));
}

fn generate_scryfall_image_link(set: &String, cn: &String) -> String {
    return format!("https://api.scryfall.com/cards/{set}/{cn}?format=image");
}

fn create_card_embeds(consolidated_results: &Vec<SearchResultEmbed>) -> Vec<CreateEmbed> {
    let mut embeds: Vec<CreateEmbed> = Vec::new();

    for result in consolidated_results {
        embeds.push(
            CreateEmbed::new()
                .title(&result.title)
                .description(format!("*${} (Card Kingdom)*",result.ck_price))
                .url(format!("https://scryfall.com/card/{}/{}/{}",result.set,result.cn,urlencoding::encode(&result.name.to_lowercase().replace(" ","-"))))
                .thumbnail(generate_scryfall_image_link(&result.set, &result.cn))
                .field("Owner",result.owners.join("\n"),true)
                .field("Quantity", result.quantities.join("\n"),true)
        )
    }

    return embeds
}

fn create_card_compact_str(consolidated_results: &Vec<SearchResultEmbed>) -> String {
    let mut result_str: String = String::new();
    let mut counter: usize = 0;

    for result in consolidated_results {
        counter += 1;
        // create the string with scryfall page link
        let mut new_entry = String::from(
            format!(
                "{}:\n*${} (Card Kingdom)*\n",
                generate_scryfall_page_link(&result.title, &result.name, &result.set, &result.cn),
                result.ck_price
            )
        );

        
        for (_i, (owner, quantity)) in result.owners.iter().zip(result.quantities.iter()).enumerate() {
            new_entry.push_str(&format!("`{quantity}` owned by `{owner}`\n"))
        }

        //final newline seperator
        new_entry.push_str("\n");

        if result_str.len() + new_entry.len() + 50 > EMBED_DESCRIPTION_MAX_LEN.into() {
            result_str.push_str(&format!("...\n\n*{} additional results truncated*",consolidated_results.len()-counter));
            break;
        } else {
            result_str.push_str(&new_entry);
        }
    }

    return result_str;
}

pub async fn search_collections(search_term: String, config: &BotConfig) -> EditInteractionResponse {
    log::info!("Searching all known collections for search term '{}'",search_term);

    let mut errors: String = String::new();
    let mut embeds: Vec<CreateEmbed>;
    let mut raw_results : Vec<SearchResultCard> = Vec::new();

    // set up all the raw collection results synchronously
    // This needs to be limited to 1 request per second to avoid rate limiting by moxfield
    let mut tick = interval(Duration::from_secs(1));
    for (i, collection) in config.mtg.collections.iter().enumerate() {
        let search_term = search_term.clone(); // Clone for each iteration here
        let result = match collection.provider {
            MTGCollectionProvider::Archidekt => {
                crate::mtg::providers::archidekt::search(
                    collection.discord_user.clone(), 
                    collection.provider_collection.clone(), 
                    search_term.clone()).await
            }
            MTGCollectionProvider::Moxfield => {
                crate::mtg::providers::moxfield::search(
                    collection.discord_user.clone(), 
                    collection.provider_collection.clone(), 
                    search_term.clone(),
                    env::var("MOXFIELD_USER_AGENT").expect("Expected MOXFIELD_USER_AGENT in the environment")).await
            }
        };
        // wait for the next tick
        match result {
            Ok(mut v) => raw_results.append(&mut v),
            Err(e) => errors.push_str(
                &format!(
                    "*Could not search collection for user `{}`: {}*\n",
                    config.mtg.collections[i].discord_user,e
                )),
        }
        tick.tick().await;
    };

    log::info!("search term '{}' completed across all collections",search_term);

    // consolidate raw results
    let consolidated_results = generate_embed_data_from_search_results(raw_results);

    if consolidated_results.len() <= EMBED_MAX_COUNT {
        // Use one embed per unique card
        embeds = create_card_embeds(&consolidated_results);
    } else {
        // use compact output method
        embeds = Vec::new();

        embeds.push(
            CreateEmbed::new()
            .title("Search Results (compact)")
            .description(
                create_card_compact_str(&consolidated_results)
            )
        );
    }

    // print out the embeds or a "no matches" message
    if consolidated_results.len() > 0 {
        return
            EditInteractionResponse::new()
                .content(&format!("Found `{}` matches in `{}` searched collection(s) for card name `{}`:\n{}",consolidated_results.len(),config.mtg.collections.len(),search_term, errors))
                .add_embeds(embeds);
    } else {
        return 
            EditInteractionResponse::new()
                .content(&format!("{}No matches found in `{}` searched collection(s) for card name `{}`", errors,config.mtg.collections.len(), search_term));
    }
}
