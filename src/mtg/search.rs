use crate::models::config::BotConfig;
use crate::mtg::models::SearchResult;
use crate::mtg::providers::archidekt::search;
use crate::models::config::MTGCollectionProvider;

pub async fn search_collections(search_term: String,config: &BotConfig) -> String {
    println!("Searching all known collections for search term {}",search_term);

    for collection in config.mtg.collections {
        let search_response = match collection.provider {
            MTGCollectionProvider::Archidekt => search(collection.discord_user,collection.provider_collection,search_term),
        };

        let mut collection_results = String::from(format!("`{}`'s collection has `{}` matches for the search term `{}`:\n", search_response.results.len(),_name));
    }
}