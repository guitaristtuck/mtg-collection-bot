use crate::models::config::{BotConfig, MTGCollectionConfig, MTGCollectionProvider};
use crate::mtg::providers::{ProviderClients, ProviderError, SearchPage};
use log;

use super::models::{SearchResultCard, SearchResultEmbed, EMBED_DESCRIPTION_MAX_LEN};
use futures::future::join_all;
use serenity::constants::EMBED_MAX_COUNT;
use std::collections::BTreeMap;

use serenity::builder::{CreateEmbed, EditInteractionResponse};
use serenity::utils::MessageBuilder;

/// Groups raw search rows by printing — `(set, collector number)`, set compared
/// case-insensitively — summing each owner's quantity, and returns the groups sorted by
/// name, then set, then collector number. Owners within a group are sorted by name.
fn generate_embed_data_from_search_results(
    search_results: Vec<SearchResultCard>,
) -> Vec<SearchResultEmbed> {
    // BTreeMap keyed by printing; rows keep their input order within a group so the
    // first-seen name is used for the group (providers may disagree on DFC names).
    let mut printings: BTreeMap<(String, String), Vec<SearchResultCard>> = BTreeMap::new();
    for item in search_results {
        printings
            .entry((item.set.to_lowercase(), item.cn.clone()))
            .or_default()
            .push(item);
    }

    let mut results: Vec<SearchResultEmbed> = printings
        .into_iter()
        .map(|((set, cn), cards)| {
            let mut quantities_by_owner: BTreeMap<&str, i64> = BTreeMap::new();
            for card in &cards {
                *quantities_by_owner.entry(&card.owner).or_insert(0) += card.quantity;
            }
            let (owners, quantities) = quantities_by_owner
                .into_iter()
                .map(|(owner, quantity)| (owner.to_string(), quantity.to_string()))
                .unzip();

            let name = cards[0].name.clone();
            SearchResultEmbed {
                title: format!("{} [{}:{}]", name, set.to_uppercase(), cn),
                ck_price: preferred_price(cards.iter().map(|c| c.ck_price)),
                name,
                set,
                cn,
                owners,
                quantities,
            }
        })
        .collect();

    results.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| a.set.cmp(&b.set))
            .then_with(|| collector_number_key(&a.cn).cmp(&collector_number_key(&b.cn)))
    });

    results
}

/// Picks the price to show for a printing whose rows may be a mix of foil and non-foil.
///
/// Rows carry no foil flag. Providers report the non-foil Card Kingdom price when it exists
/// and fall back to foil/etched otherwise (`None` when nothing is known), so the lowest known
/// price in the group is the best available stand-in for "the non-foil price".
fn preferred_price(prices: impl Iterator<Item = Option<f64>>) -> Option<f64> {
    prices.flatten().reduce(f64::min)
}

/// Sort key so collector numbers order numerically ("99" < "100"), with any
/// non-numeric suffix ("263a", "★") breaking ties lexically.
fn collector_number_key(cn: &str) -> (u64, &str) {
    let digits = cn.chars().take_while(char::is_ascii_digit).count();
    (cn[..digits].parse().unwrap_or(u64::MAX), cn)
}

/// Scryfall card page for a printing. Scryfall redirects `/card/{set}/{cn}` to the full
/// page, so no name slug is needed (and none can break on names like `A // B`).
fn scryfall_card_url(set: &str, cn: &str) -> String {
    format!("https://scryfall.com/card/{}/{}", set.to_lowercase(), cn)
}

/// The italic Card Kingdom price line, or `None` when the price is unknown.
fn ck_price_line(ck_price: Option<f64>) -> Option<String> {
    ck_price.map(|price| format!("*${price:.2} (Card Kingdom)*"))
}

/// Shown when a provider had more matches than the one page we fetch.
fn incomplete_results_line(discord_user: &str) -> String {
    MessageBuilder::new()
        .push("*Results from ")
        .push_mono_safe(discord_user)
        .push_line("'s collection may be incomplete: only the first page of matches was returned.*")
        .build()
}

fn generate_scryfall_image_link(set: &str, cn: &str) -> String {
    format!("https://api.scryfall.com/cards/{set}/{cn}?format=image")
}

fn create_card_embeds(consolidated_results: &[SearchResultEmbed]) -> Vec<CreateEmbed> {
    consolidated_results
        .iter()
        .map(|result| {
            let mut embed = CreateEmbed::new();
            if let Some(price_line) = ck_price_line(result.ck_price) {
                embed = embed.description(price_line);
            }
            embed
                .title(&result.title)
                .url(scryfall_card_url(&result.set, &result.cn))
                .thumbnail(generate_scryfall_image_link(&result.set, &result.cn))
                .field("Owner", result.owners.join("\n"), true)
                .field("Quantity", result.quantities.join("\n"), true)
        })
        .collect()
}

fn create_card_compact_str(consolidated_results: &[SearchResultEmbed]) -> String {
    // room reserved for the truncation marker
    const TRUNCATION_RESERVE: usize = 50;
    let mut result_str: String = String::new();

    for (shown, result) in consolidated_results.iter().enumerate() {
        let mut new_entry = format!(
            "[{}]({}):\n",
            result.title,
            scryfall_card_url(&result.set, &result.cn),
        );
        if let Some(price_line) = ck_price_line(result.ck_price) {
            new_entry.push_str(&price_line);
            new_entry.push('\n');
        }

        for (owner, quantity) in result.owners.iter().zip(result.quantities.iter()) {
            new_entry.push_str(&format!("`{quantity}` owned by `{owner}`\n"))
        }

        //final newline seperator
        new_entry.push('\n');

        // byte length is an upper bound on Discord's character count
        if result_str.len() + new_entry.len() + TRUNCATION_RESERVE
            > EMBED_DESCRIPTION_MAX_LEN.into()
        {
            result_str.push_str(&format!(
                "...\n\n*{} additional results truncated*",
                consolidated_results.len() - shown
            ));
            break;
        }
        result_str.push_str(&new_entry);
    }

    result_str
}

/// One line describing a collection that could not be searched. The error text comes from
/// providers/HTTP and may contain markdown, so it is escaped.
fn collection_error_line(discord_user: &str, error: &str) -> String {
    MessageBuilder::new()
        .push("*Could not search collection for user ")
        .push_mono_safe(discord_user)
        .push(": ")
        .push_safe(error)
        .push_line("*")
        .build()
}

/// The reply's message content. `search_term` is user input and is escaped.
fn search_summary_content(
    match_count: usize,
    collection_count: usize,
    search_term: &str,
    errors: &str,
) -> String {
    let mut message = MessageBuilder::new();
    if match_count > 0 {
        message
            .push("Found ")
            .push_mono(match_count.to_string())
            .push(" matches in ")
            .push_mono(collection_count.to_string())
            .push(" searched collection(s) for card name ")
            .push_mono_safe(search_term)
            .push_line(":")
            .push(errors);
    } else {
        message
            .push(errors)
            .push("No matches found in ")
            .push_mono(collection_count.to_string())
            .push(" searched collection(s) for card name ")
            .push_mono_safe(search_term);
    }
    message.build()
}

/// What searching one configured collection produced.
pub struct CollectionSearch {
    /// Discord user the collection belongs to (used in notices and as card owner).
    pub discord_user: String,
    pub result: Result<SearchPage, ProviderError>,
}

/// Search every collection for `search_term`, returning one entry per collection in
/// `collections` order.
///
/// All collections are searched concurrently: Archidekt requests go out at once, while
/// Moxfield requests queue on the Moxfield client's shared 1 request/second limiter
/// (in config order). Nothing waits after the last request.
pub async fn fetch_collection_searches(
    search_term: &str,
    collections: &[MTGCollectionConfig],
    providers: &ProviderClients,
) -> Vec<CollectionSearch> {
    let searches = collections.iter().map(|collection| async move {
        let (id, owner) = (&collection.provider_collection, &collection.discord_user);
        let result = match collection.provider {
            MTGCollectionProvider::Archidekt => {
                providers.archidekt.search(id, search_term, owner).await
            }
            MTGCollectionProvider::Moxfield => {
                providers.moxfield.search(id, search_term, owner).await
            }
        };
        CollectionSearch {
            discord_user: owner.clone(),
            result,
        }
    });
    join_all(searches).await
}

/// Build the `/mtg collections search` reply from the per-collection outcomes, in order.
///
/// The number of collections searched is `searches.len()`. Failed collections and
/// possibly-incomplete pages become notice lines in the message content; matching cards
/// are grouped by printing and shown as one embed each (up to Discord's 10-embed limit)
/// or as a single compact embed beyond that.
pub fn build_search_reply(
    search_term: &str,
    searches: Vec<CollectionSearch>,
) -> EditInteractionResponse {
    let collection_count = searches.len();
    let mut errors: String = String::new();
    let mut raw_results: Vec<SearchResultCard> = Vec::new();

    for search in searches {
        match search.result {
            Ok(mut page) => {
                if page.may_be_incomplete {
                    errors.push_str(&incomplete_results_line(&search.discord_user));
                }
                raw_results.append(&mut page.cards)
            }
            Err(e) => errors.push_str(&collection_error_line(&search.discord_user, &e.to_string())),
        }
    }

    // consolidate raw results
    let consolidated_results = generate_embed_data_from_search_results(raw_results);

    // print out the embeds or a "no matches" message
    let content = search_summary_content(
        consolidated_results.len(),
        collection_count,
        search_term,
        &errors,
    );
    if consolidated_results.is_empty() {
        return EditInteractionResponse::new().content(content);
    }

    let embeds = if consolidated_results.len() <= EMBED_MAX_COUNT {
        // Use one embed per unique card
        create_card_embeds(&consolidated_results)
    } else {
        // use compact output method
        vec![CreateEmbed::new()
            .title("Search Results (compact)")
            .description(create_card_compact_str(&consolidated_results))]
    };
    EditInteractionResponse::new()
        .content(content)
        .add_embeds(embeds)
}

/// `/mtg collections search`: fetch from every configured collection, then build the reply.
pub async fn search_collections(
    search_term: String,
    config: &BotConfig,
) -> EditInteractionResponse {
    log::info!(
        "Searching all known collections for search term '{}'",
        search_term
    );
    let searches =
        fetch_collection_searches(&search_term, &config.mtg.collections, &config.providers).await;
    log::info!(
        "search term '{}' completed across all collections",
        search_term
    );
    build_search_reply(&search_term, searches)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn card(
        name: &str,
        set: &str,
        cn: &str,
        quantity: i64,
        owner: &str,
        price: Option<f64>,
    ) -> SearchResultCard {
        SearchResultCard {
            name: name.to_string(),
            set: set.to_string(),
            cn: cn.to_string(),
            quantity,
            owner: owner.to_string(),
            ck_price: price,
        }
    }

    fn holdings(result: &SearchResultEmbed) -> Vec<(String, String)> {
        result
            .owners
            .iter()
            .cloned()
            .zip(result.quantities.iter().cloned())
            .collect()
    }

    fn pairs(v: &[(&str, &str)]) -> Vec<(String, String)> {
        v.iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect()
    }

    fn embed_json(embed: &CreateEmbed) -> Value {
        serde_json::to_value(embed).unwrap()
    }

    // --- grouping ---

    #[test]
    fn empty_input_produces_no_results() {
        let results = generate_embed_data_from_search_results(vec![]);
        assert!(results.is_empty());
        assert!(create_card_embeds(&results).is_empty());
        assert_eq!(create_card_compact_str(&results), "");
    }

    #[test]
    fn single_card_produces_single_result() {
        let results = generate_embed_data_from_search_results(vec![card(
            "Sol Ring",
            "c21",
            "263",
            2,
            "alice",
            Some(1.99),
        )]);
        assert_eq!(results.len(), 1);
        let r = &results[0];
        assert_eq!(r.title, "Sol Ring [C21:263]");
        assert_eq!(r.name, "Sol Ring");
        assert_eq!(r.set, "c21");
        assert_eq!(r.cn, "263");
        assert_eq!(r.ck_price, Some(1.99));
        assert_eq!(holdings(r), pairs(&[("alice", "2")]));
    }

    #[test]
    fn one_owner_across_several_collections_sums_quantities() {
        let results = generate_embed_data_from_search_results(vec![
            card("Sol Ring", "c21", "263", 2, "alice", Some(1.99)),
            card("Sol Ring", "c21", "263", 3, "alice", Some(1.99)),
        ]);
        assert_eq!(results.len(), 1);
        assert_eq!(holdings(&results[0]), pairs(&[("alice", "5")]));
    }

    #[test]
    fn several_owners_are_paired_with_their_quantities_in_sorted_order() {
        let results = generate_embed_data_from_search_results(vec![
            card("Sol Ring", "c21", "263", 1, "carol", Some(1.99)),
            card("Sol Ring", "c21", "263", 7, "alice", Some(1.99)),
            card("Sol Ring", "c21", "263", 4, "bob", Some(1.99)),
            card("Sol Ring", "c21", "263", 2, "carol", Some(1.99)),
        ]);
        assert_eq!(results.len(), 1);
        assert_eq!(
            holdings(&results[0]),
            pairs(&[("alice", "7"), ("bob", "4"), ("carol", "3")])
        );
    }

    #[test]
    fn set_codes_differing_only_in_case_are_one_printing() {
        let results = generate_embed_data_from_search_results(vec![
            card("Sol Ring", "C21", "263", 1, "alice", Some(1.99)),
            card("Sol Ring", "c21", "263", 1, "bob", Some(1.99)),
        ]);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Sol Ring [C21:263]");
        assert_eq!(results[0].set, "c21");
        assert_eq!(
            holdings(&results[0]),
            pairs(&[("alice", "1"), ("bob", "1")])
        );
    }

    #[test]
    fn card_name_does_not_split_a_printing() {
        // Providers can disagree on a double-faced card's name.
        let results = generate_embed_data_from_search_results(vec![
            card(
                "Delver of Secrets // Insectile Aberration",
                "isd",
                "51",
                1,
                "alice",
                Some(0.25),
            ),
            card("Delver of Secrets", "isd", "51", 1, "bob", Some(0.25)),
        ]);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "Delver of Secrets // Insectile Aberration");
    }

    #[test]
    fn foil_and_non_foil_rows_merge_and_prefer_non_foil_price() {
        // Providers report the non-foil price when it exists and fall back to the
        // foil price (or None) otherwise, so the lowest known price in a group
        // is the non-foil one.
        let results = generate_embed_data_from_search_results(vec![
            card("Sol Ring", "c21", "263", 1, "alice", None),
            card("Sol Ring", "c21", "263", 1, "alice", Some(5.49)),
            card("Sol Ring", "c21", "263", 2, "bob", Some(1.99)),
        ]);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].ck_price, Some(1.99));
        assert_eq!(
            holdings(&results[0]),
            pairs(&[("alice", "2"), ("bob", "2")])
        );
    }

    #[test]
    fn price_is_none_when_no_row_has_one() {
        let results = generate_embed_data_from_search_results(vec![
            card("Sol Ring", "c21", "263", 1, "alice", None),
            card("Sol Ring", "c21", "263", 1, "bob", None),
        ]);
        assert_eq!(results[0].ck_price, None);
    }

    #[test]
    fn group_price_ignores_rows_without_a_price() {
        let results = generate_embed_data_from_search_results(vec![
            card("Sol Ring", "c21", "263", 1, "alice", None),
            card("Sol Ring", "c21", "263", 1, "bob", Some(2.49)),
        ]);
        assert_eq!(results[0].ck_price, Some(2.49));
    }

    #[test]
    fn results_are_sorted_by_name_then_set_then_collector_number() {
        let results = generate_embed_data_from_search_results(vec![
            card("Sol Ring", "cmr", "472", 1, "a", Some(1.0)),
            card("Sol Ring", "c21", "263", 1, "a", Some(1.0)),
            card("Arcane Signet", "cmr", "297", 1, "a", Some(1.0)),
            card("Sol Ring", "c21", "1000", 1, "a", Some(1.0)),
            card("Sol Ring", "c21", "99", 1, "a", Some(1.0)),
            card("sol ring", "C21", "100", 1, "a", Some(1.0)),
        ]);
        let titles: Vec<&str> = results.iter().map(|r| r.title.as_str()).collect();
        assert_eq!(
            titles,
            vec![
                "Arcane Signet [CMR:297]",
                "Sol Ring [C21:99]",
                "sol ring [C21:100]",
                "Sol Ring [C21:263]",
                "Sol Ring [C21:1000]",
                "Sol Ring [CMR:472]",
            ]
        );
    }

    // --- links ---

    #[test]
    fn scryfall_url_uses_lowercase_set_and_collector_number_only() {
        assert_eq!(
            scryfall_card_url("C21", "263"),
            "https://scryfall.com/card/c21/263"
        );
    }

    #[test]
    fn double_faced_card_links_have_no_name_slug() {
        let results = generate_embed_data_from_search_results(vec![card(
            "Delver of Secrets // Insectile Aberration",
            "ISD",
            "51",
            1,
            "alice",
            Some(0.25),
        )]);
        let embed = embed_json(&create_card_embeds(&results)[0]);
        assert_eq!(embed["url"], "https://scryfall.com/card/isd/51");

        let compact = create_card_compact_str(&results);
        assert!(
            compact.contains("(https://scryfall.com/card/isd/51)"),
            "{compact}"
        );
        assert!(!compact.contains("delver"), "{compact}");
    }

    // --- embeds ---

    #[test]
    fn embed_fields_are_serialized_correctly() {
        let results = generate_embed_data_from_search_results(vec![
            card("Sol Ring", "c21", "263", 2, "bob", Some(1.99)),
            card("Sol Ring", "c21", "263", 1, "alice", Some(1.99)),
        ]);
        let embeds = create_card_embeds(&results);
        assert_eq!(embeds.len(), 1);
        let embed = embed_json(&embeds[0]);
        assert_eq!(embed["title"], "Sol Ring [C21:263]");
        assert_eq!(embed["description"], "*$1.99 (Card Kingdom)*");
        assert_eq!(embed["url"], "https://scryfall.com/card/c21/263");
        assert_eq!(
            embed["thumbnail"]["url"],
            "https://api.scryfall.com/cards/c21/263?format=image"
        );
        assert_eq!(
            embed["fields"],
            json!([
                {"name": "Owner", "value": "alice\nbob", "inline": true},
                {"name": "Quantity", "value": "1\n2", "inline": true},
            ])
        );
    }

    // --- compact ---

    #[test]
    fn compact_entry_format() {
        let results = generate_embed_data_from_search_results(vec![
            card("Sol Ring", "c21", "263", 2, "bob", Some(1.99)),
            card("Sol Ring", "c21", "263", 1, "alice", Some(1.99)),
        ]);
        assert_eq!(
            create_card_compact_str(&results),
            "[Sol Ring [C21:263]](https://scryfall.com/card/c21/263):\n\
             *$1.99 (Card Kingdom)*\n\
             `1` owned by `alice`\n\
             `2` owned by `bob`\n\n"
        );
    }

    fn many_cards(count: usize, name_prefix: &str) -> Vec<SearchResultEmbed> {
        generate_embed_data_from_search_results(
            (0..count)
                .map(|i| {
                    card(
                        &format!("{name_prefix} {i:04}"),
                        "c21",
                        &i.to_string(),
                        1,
                        "alice",
                        Some(1.99),
                    )
                })
                .collect(),
        )
    }

    fn assert_truncation_count_is_correct(results: &[SearchResultEmbed]) {
        let compact = create_card_compact_str(results);
        assert!(
            compact.chars().count() <= EMBED_DESCRIPTION_MAX_LEN as usize,
            "compact output is {} chars",
            compact.chars().count()
        );
        let shown = compact.matches("owned by").count();
        let marker = compact
            .rsplit_once("...\n\n*")
            .map(|(_, tail)| tail)
            .expect("expected truncation marker");
        let truncated: usize = marker
            .strip_suffix(" additional results truncated*")
            .expect("expected truncation suffix")
            .parse()
            .unwrap();
        assert!(shown > 0);
        assert_eq!(shown + truncated, results.len());
    }

    #[test]
    fn compact_output_is_not_truncated_when_small() {
        let results = many_cards(5, "Card");
        let compact = create_card_compact_str(&results);
        assert!(!compact.contains("truncated"));
        assert_eq!(compact.matches("owned by").count(), 5);
    }

    #[test]
    fn compact_truncation_count_includes_the_overflowing_entry() {
        assert_truncation_count_is_correct(&many_cards(200, "Card"));
    }

    #[test]
    fn compact_truncation_with_multibyte_names() {
        assert_truncation_count_is_correct(&many_cards(200, "Æther Vial Ünïcödé 漢字"));
    }

    #[test]
    fn compact_truncation_at_exact_boundary() {
        // Find the largest number of entries that fits untruncated, then add one.
        let mut fits = 0;
        for n in 1..200 {
            if create_card_compact_str(&many_cards(n, "Card")).contains("truncated") {
                break;
            }
            fits = n;
        }
        assert!(fits > 0 && fits < 199);
        let results = many_cards(fits + 1, "Card");
        let compact = create_card_compact_str(&results);
        assert!(
            compact.ends_with("...\n\n*1 additional results truncated*"),
            "{compact}"
        );
        assert_truncation_count_is_correct(&results);
    }

    // --- escaping ---

    #[test]
    fn search_term_is_escaped_in_reply_content() {
        assert_eq!(
            search_summary_content(3, 2, "sol` **ring** @everyone", ""),
            "Found `3` matches in `2` searched collection(s) for card name \
             `sol' **ring** @\u{200B}everyone`:\n"
        );
        assert_eq!(
            search_summary_content(0, 2, "`_x_`", ""),
            "No matches found in `2` searched collection(s) for card name `'_x_'`"
        );
    }

    #[test]
    fn errors_are_included_in_reply_content() {
        let err = collection_error_line("alice", "bad");
        assert_eq!(
            search_summary_content(1, 2, "sol", &err),
            format!("Found `1` matches in `2` searched collection(s) for card name `sol`:\n{err}")
        );
        assert_eq!(
            search_summary_content(0, 2, "sol", &err),
            format!("{err}No matches found in `2` searched collection(s) for card name `sol`")
        );
    }

    #[test]
    fn error_text_is_escaped() {
        assert_eq!(
            collection_error_line("al`ice", "error decoding `*_body_*`"),
            "*Could not search collection for user `al'ice`: \
             error decoding \\`\\*\\_body\\_\\*\\`*\n"
        );
    }
}

#[cfg(test)]
mod price_line_tests {
    use super::*;

    fn result(ck_price: Option<f64>) -> SearchResultEmbed {
        SearchResultEmbed {
            title: "Sol Ring [SLD:2807]".to_string(),
            name: "Sol Ring".to_string(),
            set: "sld".to_string(),
            cn: "2807".to_string(),
            owners: vec!["tuckface".to_string()],
            quantities: vec!["1".to_string()],
            ck_price,
        }
    }

    fn embed_json(embed: &CreateEmbed) -> serde_json::Value {
        serde_json::to_value(embed).unwrap()
    }

    #[test]
    fn price_line_formats_two_decimals() {
        assert_eq!(
            ck_price_line(Some(7.5)).as_deref(),
            Some("*$7.50 (Card Kingdom)*")
        );
        assert_eq!(ck_price_line(None), None);
    }

    #[test]
    fn embed_has_price_description_when_known() {
        let embeds = create_card_embeds(&[result(Some(7.49))]);
        assert_eq!(
            embed_json(&embeds[0])["description"],
            "*$7.49 (Card Kingdom)*"
        );
    }

    #[test]
    fn embed_omits_price_when_unknown() {
        let embeds = create_card_embeds(&[result(None)]);
        let json = embed_json(&embeds[0]);
        assert!(json.get("description").is_none(), "{json}");
    }

    #[test]
    fn compact_entry_omits_price_when_unknown() {
        let text = create_card_compact_str(&[result(None)]);
        assert!(!text.contains("Card Kingdom"), "{text}");
        assert!(!text.contains("$0.00"), "{text}");
        assert!(text.contains("`1` owned by `tuckface`"), "{text}");
    }

    #[test]
    fn compact_entry_includes_price_when_known() {
        let text = create_card_compact_str(&[result(Some(1.29))]);
        assert!(
            text.contains(":\n*$1.29 (Card Kingdom)*\n`1` owned by"),
            "{text}"
        );
    }

    #[test]
    fn incomplete_line_names_the_user() {
        assert_eq!(
            incomplete_results_line("wookiee"),
            "*Results from `wookiee`'s collection may be incomplete: \
             only the first page of matches was returned.*\n"
        );
        // user names are escaped like in error lines
        assert!(incomplete_results_line("a`b").contains("`a'b`"));
    }
}

#[cfg(test)]
mod reply_tests {
    use super::*;
    use serde_json::Value;
    use std::ops::Range;

    fn card(name: &str, cn: &str, owner: &str) -> SearchResultCard {
        SearchResultCard {
            name: name.to_string(),
            set: "c21".to_string(),
            cn: cn.to_string(),
            quantity: 1,
            owner: owner.to_string(),
            ck_price: Some(1.0),
        }
    }

    /// Distinct printings `Card NN [C21:N]` for each N in `range`, owned by `owner`.
    fn distinct_cards(range: Range<usize>, owner: &str) -> Vec<SearchResultCard> {
        range
            .map(|i| card(&format!("Card {i:02}"), &i.to_string(), owner))
            .collect()
    }

    fn page(
        owner: &str,
        cards: Vec<SearchResultCard>,
        may_be_incomplete: bool,
    ) -> CollectionSearch {
        CollectionSearch {
            discord_user: owner.to_string(),
            result: Ok(SearchPage {
                cards,
                may_be_incomplete,
            }),
        }
    }

    fn found(owner: &str, cards: Vec<SearchResultCard>) -> CollectionSearch {
        page(owner, cards, false)
    }

    fn failed(owner: &str) -> CollectionSearch {
        CollectionSearch {
            discord_user: owner.to_string(),
            result: Err(ProviderError::Status {
                what: "Moxfield collection search",
                status: reqwest::StatusCode::TOO_MANY_REQUESTS,
            }),
        }
    }

    fn reply_json(searches: Vec<CollectionSearch>) -> Value {
        serde_json::to_value(build_search_reply("sol", searches)).unwrap()
    }

    fn content(reply: &Value) -> &str {
        reply["content"].as_str().unwrap()
    }

    fn embeds(reply: &Value) -> &Vec<Value> {
        reply["embeds"].as_array().expect("expected embeds")
    }

    const FAILED_BOB: &str = "*Could not search collection for user `bob`: \
        Moxfield collection search failed with status code 429 Too Many Requests*\n";

    #[test]
    fn no_matches_has_message_and_no_embeds() {
        let reply = reply_json(vec![found("alice", vec![]), found("bob", vec![])]);
        assert_eq!(
            content(&reply),
            "No matches found in `2` searched collection(s) for card name `sol`"
        );
        assert!(reply.get("embeds").is_none(), "{reply}");
    }

    #[test]
    fn no_collections_is_a_no_match_reply() {
        let reply = reply_json(vec![]);
        assert_eq!(
            content(&reply),
            "No matches found in `0` searched collection(s) for card name `sol`"
        );
        assert!(reply.get("embeds").is_none(), "{reply}");
    }

    #[test]
    fn single_match_is_one_card_embed() {
        let reply = reply_json(vec![found("alice", vec![card("Sol Ring", "263", "alice")])]);
        assert_eq!(
            content(&reply),
            "Found `1` matches in `1` searched collection(s) for card name `sol`:\n"
        );
        let embeds = embeds(&reply);
        assert_eq!(embeds.len(), 1);
        assert_eq!(embeds[0]["title"], "Sol Ring [C21:263]");
    }

    #[test]
    fn ten_results_are_individual_embeds() {
        let reply = reply_json(vec![
            found("alice", distinct_cards(0..6, "alice")),
            found("bob", distinct_cards(6..10, "bob")),
        ]);
        assert!(
            content(&reply).starts_with("Found `10` matches in `2` searched"),
            "{reply}"
        );
        let embeds = embeds(&reply);
        assert_eq!(embeds.len(), 10);
        assert!(embeds.iter().all(|e| e["fields"].is_array()), "{reply}");
        assert_eq!(embeds[0]["title"], "Card 00 [C21:0]");
        assert_eq!(embeds[9]["title"], "Card 09 [C21:9]");
    }

    #[test]
    fn eleven_results_become_one_compact_embed() {
        let reply = reply_json(vec![found("alice", distinct_cards(0..11, "alice"))]);
        assert!(
            content(&reply).starts_with("Found `11` matches in `1` searched"),
            "{reply}"
        );
        let embeds = embeds(&reply);
        assert_eq!(embeds.len(), 1);
        assert_eq!(embeds[0]["title"], "Search Results (compact)");
        let description = embeds[0]["description"].as_str().unwrap();
        assert_eq!(description.matches("owned by `alice`").count(), 11);
    }

    #[test]
    fn same_printing_from_several_collections_is_one_result() {
        let reply = reply_json(vec![
            found("alice", vec![card("Sol Ring", "263", "alice")]),
            found("bob", vec![card("Sol Ring", "263", "bob")]),
        ]);
        assert!(
            content(&reply).starts_with("Found `1` matches in `2` searched"),
            "{reply}"
        );
        assert_eq!(embeds(&reply).len(), 1);
    }

    #[test]
    fn failed_collection_adds_error_line_and_still_counts_as_searched() {
        let reply = reply_json(vec![
            found("alice", vec![card("Sol Ring", "263", "alice")]),
            failed("bob"),
        ]);
        assert_eq!(
            content(&reply),
            format!(
                "Found `1` matches in `2` searched collection(s) for card name `sol`:\n{FAILED_BOB}"
            )
        );
    }

    #[test]
    fn error_lines_precede_the_no_matches_message() {
        let reply = reply_json(vec![found("alice", vec![]), failed("bob")]);
        assert_eq!(
            content(&reply),
            format!(
                "{FAILED_BOB}No matches found in `2` searched collection(s) for card name `sol`"
            )
        );
        assert!(reply.get("embeds").is_none(), "{reply}");
    }

    #[test]
    fn notices_keep_collection_order() {
        let reply = reply_json(vec![
            failed("bob"),
            page("carol", vec![card("Sol Ring", "263", "carol")], true),
            failed("bob"),
        ]);
        let carol_incomplete = incomplete_results_line("carol");
        assert_eq!(
            content(&reply),
            format!(
                "Found `1` matches in `3` searched collection(s) for card name `sol`:\n\
                 {FAILED_BOB}{carol_incomplete}{FAILED_BOB}"
            )
        );
    }
}
