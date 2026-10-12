use crate::models::config::BotConfig;
use crate::models::config::MTGCollectionProvider;
use log;

use super::models::{SearchResultCard, SearchResultEmbed, EMBED_DESCRIPTION_MAX_LEN};
use serenity::constants::EMBED_MAX_COUNT;
use std::collections::BTreeMap;
use tokio::time::{interval, Duration};

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
                ck_price: preferred_price(cards.iter().map(|c| c.ck_price.as_str())),
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
/// Rows only carry a price string. Providers already report the non-foil Card Kingdom price
/// when it exists and fall back to foil/etched (or `0.00`) otherwise, so the lowest non-zero
/// price in the group is the best available stand-in for "the non-foil price".
fn preferred_price<'a>(prices: impl Iterator<Item = &'a str>) -> String {
    let mut fallback: Option<&str> = None;
    let mut best: Option<(f64, &str)> = None;
    for price in prices {
        fallback.get_or_insert(price);
        if let Ok(value) = price.parse::<f64>() {
            if value > 0.0 && best.is_none_or(|(b, _)| value < b) {
                best = Some((value, price));
            }
        }
    }
    best.map(|(_, p)| p)
        .or(fallback)
        .unwrap_or_default()
        .to_string()
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

fn generate_scryfall_image_link(set: &str, cn: &str) -> String {
    format!("https://api.scryfall.com/cards/{set}/{cn}?format=image")
}

fn create_card_embeds(consolidated_results: &[SearchResultEmbed]) -> Vec<CreateEmbed> {
    consolidated_results
        .iter()
        .map(|result| {
            CreateEmbed::new()
                .title(&result.title)
                .description(format!("*${} (Card Kingdom)*", result.ck_price))
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
            "[{}]({}):\n*${} (Card Kingdom)*\n",
            result.title,
            scryfall_card_url(&result.set, &result.cn),
            result.ck_price
        );

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

pub async fn search_collections(
    search_term: String,
    config: &BotConfig,
) -> EditInteractionResponse {
    log::info!(
        "Searching all known collections for search term '{}'",
        search_term
    );

    let mut errors: String = String::new();
    let mut raw_results: Vec<SearchResultCard> = Vec::new();

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
                    search_term.clone(),
                )
                .await
            }
            MTGCollectionProvider::Moxfield => {
                crate::mtg::providers::moxfield::search(
                    collection.discord_user.clone(),
                    collection.provider_collection.clone(),
                    search_term.clone(),
                    config.env.moxfield_user_agent.clone(),
                )
                .await
            }
        };
        // wait for the next tick
        match result {
            Ok(mut v) => raw_results.append(&mut v),
            Err(e) => errors.push_str(&collection_error_line(
                &config.mtg.collections[i].discord_user,
                &e.to_string(),
            )),
        }
        tick.tick().await;
    }

    log::info!(
        "search term '{}' completed across all collections",
        search_term
    );

    // consolidate raw results
    let consolidated_results = generate_embed_data_from_search_results(raw_results);

    let embeds = if consolidated_results.len() <= EMBED_MAX_COUNT {
        // Use one embed per unique card
        create_card_embeds(&consolidated_results)
    } else {
        // use compact output method
        vec![CreateEmbed::new()
            .title("Search Results (compact)")
            .description(create_card_compact_str(&consolidated_results))]
    };

    // print out the embeds or a "no matches" message
    let content = search_summary_content(
        consolidated_results.len(),
        config.mtg.collections.len(),
        &search_term,
        &errors,
    );
    if !consolidated_results.is_empty() {
        EditInteractionResponse::new()
            .content(content)
            .add_embeds(embeds)
    } else {
        EditInteractionResponse::new().content(content)
    }
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
        price: &str,
    ) -> SearchResultCard {
        SearchResultCard {
            name: name.to_string(),
            set: set.to_string(),
            cn: cn.to_string(),
            quantity,
            owner: owner.to_string(),
            ck_price: price.to_string(),
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
            "Sol Ring", "c21", "263", 2, "alice", "1.99",
        )]);
        assert_eq!(results.len(), 1);
        let r = &results[0];
        assert_eq!(r.title, "Sol Ring [C21:263]");
        assert_eq!(r.name, "Sol Ring");
        assert_eq!(r.set, "c21");
        assert_eq!(r.cn, "263");
        assert_eq!(r.ck_price, "1.99");
        assert_eq!(holdings(r), pairs(&[("alice", "2")]));
    }

    #[test]
    fn one_owner_across_several_collections_sums_quantities() {
        let results = generate_embed_data_from_search_results(vec![
            card("Sol Ring", "c21", "263", 2, "alice", "1.99"),
            card("Sol Ring", "c21", "263", 3, "alice", "1.99"),
        ]);
        assert_eq!(results.len(), 1);
        assert_eq!(holdings(&results[0]), pairs(&[("alice", "5")]));
    }

    #[test]
    fn several_owners_are_paired_with_their_quantities_in_sorted_order() {
        let results = generate_embed_data_from_search_results(vec![
            card("Sol Ring", "c21", "263", 1, "carol", "1.99"),
            card("Sol Ring", "c21", "263", 7, "alice", "1.99"),
            card("Sol Ring", "c21", "263", 4, "bob", "1.99"),
            card("Sol Ring", "c21", "263", 2, "carol", "1.99"),
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
            card("Sol Ring", "C21", "263", 1, "alice", "1.99"),
            card("Sol Ring", "c21", "263", 1, "bob", "1.99"),
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
                "0.25",
            ),
            card("Delver of Secrets", "isd", "51", 1, "bob", "0.25"),
        ]);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "Delver of Secrets // Insectile Aberration");
    }

    #[test]
    fn foil_and_non_foil_rows_merge_and_prefer_non_foil_price() {
        // Providers report the non-foil price when it exists and fall back to the
        // foil price (or 0.00) otherwise, so the lowest non-zero price in a group
        // is the non-foil one.
        let results = generate_embed_data_from_search_results(vec![
            card("Sol Ring", "c21", "263", 1, "alice", "0.00"),
            card("Sol Ring", "c21", "263", 1, "alice", "5.49"),
            card("Sol Ring", "c21", "263", 2, "bob", "1.99"),
        ]);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].ck_price, "1.99");
        assert_eq!(
            holdings(&results[0]),
            pairs(&[("alice", "2"), ("bob", "2")])
        );
    }

    #[test]
    fn price_falls_back_to_zero_when_nothing_else_is_known() {
        let results = generate_embed_data_from_search_results(vec![card(
            "Sol Ring", "c21", "263", 1, "alice", "0.00",
        )]);
        assert_eq!(results[0].ck_price, "0.00");
    }

    #[test]
    fn results_are_sorted_by_name_then_set_then_collector_number() {
        let results = generate_embed_data_from_search_results(vec![
            card("Sol Ring", "cmr", "472", 1, "a", "1"),
            card("Sol Ring", "c21", "263", 1, "a", "1"),
            card("Arcane Signet", "cmr", "297", 1, "a", "1"),
            card("Sol Ring", "c21", "1000", 1, "a", "1"),
            card("Sol Ring", "c21", "99", 1, "a", "1"),
            card("sol ring", "C21", "100", 1, "a", "1"),
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
            "0.25",
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
            card("Sol Ring", "c21", "263", 2, "bob", "1.99"),
            card("Sol Ring", "c21", "263", 1, "alice", "1.99"),
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
            card("Sol Ring", "c21", "263", 2, "bob", "1.99"),
            card("Sol Ring", "c21", "263", 1, "alice", "1.99"),
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
                        "1.99",
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
