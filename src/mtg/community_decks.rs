use crate::models::config::{BotConfig, MTGCollectionProvider};
use serenity::builder::{CreateEmbed,EditInteractionResponse};

pub async fn list_community_decks(config: &BotConfig) -> EditInteractionResponse {
    // get all needed metadata for the community decks
    log::info!("Collecting metadata for all configured community decks");
    
    let mut errors: String = String::new();
    let mut embeds: Vec<CreateEmbed> = Vec::new();

    // set up all the raw collection results asynchronously
    let futures = config.mtg.community_decks.iter().enumerate().map(|(i, deck)| {
        async move {
            let result = match deck.provider {
                MTGCollectionProvider::Archidekt => {
                    crate::mtg::providers::archidekt::get_deck(deck.discord_user.clone(), deck.provider_deck.clone()).await
                }
                MTGCollectionProvider::Moxfield => {
                    crate::mtg::providers::moxfield::get_deck(deck.discord_user.clone(), deck.provider_deck.clone()).await
                }
            };
            (i, result)
        }
    });

    // gather all the results, block until all return
    let deck_responses = futures::future::join_all(futures).await;
    log::info!("community deck metadata retrieval completed across all decks");
    

    // create an embed for each community deck
    for deck in deck_responses.into_iter() {
        match deck.1 {
            Ok(value) => {
                embeds.push(
                        CreateEmbed::new()
                            .title(value.title)
                            .url(value.url)
                            .thumbnail(value.thumbnail)
                            .field("Original Creator",value.original_owner, false)
                            .field("Last Updated At", value.last_updated_at, false)
                );
            }
            Err(e) => {
                errors.push_str(&format!("*Could not load deck for deck id `{}`: {}*\n",config.mtg.community_decks[deck.0].provider_deck,e))
            }
        }
    }

    return EditInteractionResponse::new()
            .content(&format!("Displaying `{}` of `{}` configured community decks:\n{}",embeds.len(),config.mtg.community_decks.len(), errors))
            .add_embeds(embeds);
}