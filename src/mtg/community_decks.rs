use crate::models::config::{BotConfig, MTGCollectionProvider, MTGCommunityDeck};
use crate::mtg::models::CommunityDeckMetadata;
use crate::mtg::providers::{ProviderClients, ProviderError};
use futures::future::join_all;
use serenity::builder::{CreateEmbed, EditInteractionResponse};

/// What looking up one configured community deck produced.
pub struct DeckLookup {
    /// The provider's deck ID, as configured (named in the error line on failure).
    pub deck_id: String,
    pub result: Result<CommunityDeckMetadata, ProviderError>,
}

/// Look up every community deck, returning one entry per deck in `decks` order.
///
/// All lookups run concurrently: Archidekt requests go out at once, while Moxfield
/// requests queue on the Moxfield client's shared 1 request/second limiter (in config
/// order). Nothing waits after the last request.
pub async fn fetch_community_decks(
    decks: &[MTGCommunityDeck],
    providers: &ProviderClients,
) -> Vec<DeckLookup> {
    let lookups = decks.iter().map(|deck| async move {
        let id = &deck.provider_deck;
        let result = match deck.provider {
            MTGCollectionProvider::Archidekt => providers.archidekt.get_deck(id).await,
            MTGCollectionProvider::Moxfield => providers.moxfield.get_deck(id).await,
        };
        DeckLookup {
            deck_id: id.clone(),
            result,
        }
    });
    join_all(lookups).await
}

/// Build the `/mtg community_decks list` reply: an "x of y" count, one error line per
/// failed lookup, and one embed per deck that loaded, in configured order.
pub fn build_community_decks_reply(lookups: Vec<DeckLookup>) -> EditInteractionResponse {
    let configured = lookups.len();
    let mut errors: String = String::new();
    let mut embeds: Vec<CreateEmbed> = Vec::new();

    for lookup in lookups {
        match lookup.result {
            Ok(deck) => embeds.push(deck_embed(deck)),
            Err(e) => errors.push_str(&format!(
                "*Could not load deck for deck id `{}`: {}*\n",
                lookup.deck_id, e
            )),
        }
    }

    EditInteractionResponse::new()
        .content(format!(
            "Displaying `{}` of `{}` configured community decks:\n{}",
            embeds.len(),
            configured,
            errors
        ))
        .add_embeds(embeds)
}

fn deck_embed(deck: CommunityDeckMetadata) -> CreateEmbed {
    let mut embed = CreateEmbed::new()
        .title(deck.title)
        .url(deck.url)
        .field("Original Creator", deck.original_owner, false)
        .field("Last Updated At", deck.last_updated_at, false);
    if let Some(thumbnail) = deck.thumbnail {
        embed = embed.thumbnail(thumbnail);
    }
    embed
}

/// `/mtg community_decks list`: look up every configured deck, then build the reply.
pub async fn list_community_decks(config: &BotConfig) -> EditInteractionResponse {
    log::info!("Collecting metadata for all configured community decks");
    let lookups = fetch_community_decks(&config.mtg.community_decks, &config.providers).await;
    log::info!("community deck metadata retrieval completed across all decks");
    build_community_decks_reply(lookups)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn deck(title: &str, thumbnail: Option<&str>) -> CommunityDeckMetadata {
        CommunityDeckMetadata {
            title: title.to_string(),
            url: format!("https://example.test/decks/{title}"),
            thumbnail: thumbnail.map(str::to_string),
            original_owner: "scondo62".to_string(),
            last_updated_at: "2024-04-07T14:24:25.713Z".to_string(),
        }
    }

    fn loaded(id: &str, title: &str) -> DeckLookup {
        DeckLookup {
            deck_id: id.to_string(),
            result: Ok(deck(title, Some("https://img.test/x.jpg"))),
        }
    }

    fn failed(id: &str) -> DeckLookup {
        DeckLookup {
            deck_id: id.to_string(),
            result: Err(ProviderError::Status {
                what: "Moxfield deck lookup",
                status: reqwest::StatusCode::NOT_FOUND,
            }),
        }
    }

    fn reply_json(lookups: Vec<DeckLookup>) -> Value {
        serde_json::to_value(build_community_decks_reply(lookups)).unwrap()
    }

    fn not_found_line(id: &str) -> String {
        format!(
            "*Could not load deck for deck id `{id}`: \
             Moxfield deck lookup failed with status code 404 Not Found*\n"
        )
    }

    #[test]
    fn all_decks_loaded() {
        let reply = reply_json(vec![loaded("a", "Kenrith"), loaded("b", "Atraxa")]);
        assert_eq!(
            reply["content"],
            "Displaying `2` of `2` configured community decks:\n"
        );
        let titles: Vec<&Value> = reply["embeds"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| &e["title"])
            .collect();
        assert_eq!(titles, vec!["Kenrith", "Atraxa"]);
    }

    #[test]
    fn failed_decks_are_counted_and_named_by_their_own_id() {
        let reply = reply_json(vec![
            failed("first-id"),
            loaded("ok-id", "Kenrith"),
            failed("third-id"),
        ]);
        assert_eq!(
            reply["content"],
            format!(
                "Displaying `1` of `3` configured community decks:\n{}{}",
                not_found_line("first-id"),
                not_found_line("third-id")
            )
        );
        assert_eq!(reply["embeds"].as_array().unwrap().len(), 1);
        assert_eq!(reply["embeds"][0]["title"], "Kenrith");
    }

    #[test]
    fn all_decks_failed() {
        let reply = reply_json(vec![failed("x")]);
        assert_eq!(
            reply["content"],
            format!(
                "Displaying `0` of `1` configured community decks:\n{}",
                not_found_line("x")
            )
        );
        assert_eq!(reply["embeds"], json!([]));
    }

    #[test]
    fn no_decks_configured() {
        let reply = reply_json(vec![]);
        assert_eq!(
            reply["content"],
            "Displaying `0` of `0` configured community decks:\n"
        );
    }

    #[test]
    fn deck_embed_fields() {
        let reply = reply_json(vec![loaded("a", "Kenrith")]);
        let embed = &reply["embeds"][0];
        assert_eq!(embed["url"], "https://example.test/decks/Kenrith");
        assert_eq!(embed["thumbnail"]["url"], "https://img.test/x.jpg");
        assert_eq!(
            embed["fields"],
            json!([
                {"name": "Original Creator", "value": "scondo62", "inline": false},
                {"name": "Last Updated At", "value": "2024-04-07T14:24:25.713Z", "inline": false},
            ])
        );
    }

    #[test]
    fn deck_without_thumbnail_has_no_thumbnail() {
        let reply = reply_json(vec![DeckLookup {
            deck_id: "a".to_string(),
            result: Ok(deck("Kenrith", None)),
        }]);
        assert!(reply["embeds"][0].get("thumbnail").is_none(), "{reply}");
    }
}
