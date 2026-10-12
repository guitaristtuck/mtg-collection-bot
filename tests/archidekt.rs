//! Archidekt provider: fixture mapping tests and HTTP tests against a local wiremock server.

use fb3k_discord_bot::mtg::providers::archidekt::{
    parse_deck, parse_search, ArchidektClient, ARCHIDEKT_API_BASE_URL, ARCHIDEKT_SEARCH_PAGE_SIZE,
};
use serde_json::{json, Value};
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const SEARCH_FIXTURE: &str = include_str!("fixtures/archidekt_search.json");
const DECK_FIXTURE: &str = include_str!("fixtures/archidekt_deck.json");

fn fixture(json: &str) -> Value {
    serde_json::from_str(json).unwrap()
}

fn client(server: &MockServer) -> ArchidektClient {
    ArchidektClient::with_base_url(server.uri()).unwrap()
}

// ---- search mapping ----

#[test]
fn search_fixture_maps_both_card_shapes() {
    let page = parse_search(SEARCH_FIXTURE, "tuckface").unwrap();
    assert_eq!(page.cards.len(), 4);

    // Current shape: edition.editioncode + collectorNumber
    let sol_ring = &page.cards[0];
    assert_eq!(sol_ring.name, "Sol Ring");
    assert_eq!(sol_ring.set, "sld");
    assert_eq!(sol_ring.cn, "2807");
    assert_eq!(sol_ring.quantity, 1);
    assert_eq!(sol_ring.owner, "tuckface");
    assert_eq!(sol_ring.ck_price, Some(7.49));

    // Flat shape: set + cn
    let sorin = &page.cards[1];
    assert_eq!(sorin.name, "Sorin, Solemn Visitor");
    assert_eq!(sorin.set, "ktk");
    assert_eq!(sorin.cn, "202");
    assert_eq!(sorin.ck_price, Some(1.29));
}

#[test]
fn search_fixture_maps_double_faced_card() {
    let page = parse_search(SEARCH_FIXTURE, "tuckface").unwrap();
    let delver = &page.cards[2];
    assert_eq!(delver.name, "Delver of Secrets // Insectile Aberration");
    assert_eq!((delver.set.as_str(), delver.cn.as_str()), ("inr", "60"));
    assert_eq!(delver.ck_price, Some(0.49));
}

#[test]
fn search_fixture_card_without_ck_price_has_no_price() {
    let page = parse_search(SEARCH_FIXTURE, "tuckface").unwrap();
    let plains = &page.cards[3];
    assert_eq!(plains.name, "Plains");
    assert_eq!(plains.set, "tle");
    assert_eq!(plains.ck_price, None);
}

#[test]
fn search_fixture_with_next_page_may_be_incomplete() {
    let page = parse_search(SEARCH_FIXTURE, "tuckface").unwrap();
    assert!(page.may_be_incomplete);
}

#[test]
fn empty_search_results() {
    let body = json!({"count": 0, "next": null, "previous": null, "results": []});
    let page = parse_search(&body.to_string(), "tuckface").unwrap();
    assert!(page.cards.is_empty());
    assert!(!page.may_be_incomplete);
}

#[test]
fn unknown_extra_fields_are_ignored() {
    let mut body = fixture(SEARCH_FIXTURE);
    body["somethingNew"] = json!({"nested": [1, 2, 3]});
    body["results"][0]["card"]["brandNewField"] = json!("x");
    body["results"][0]["card"]["prices"]["newVendor"] = json!(1.0);
    let page = parse_search(&body.to_string(), "tuckface").unwrap();
    assert_eq!(page.cards.len(), 4);
}

#[test]
fn malformed_card_fails_the_whole_search() {
    let mut body = fixture(SEARCH_FIXTURE);
    body["results"][1]["card"]
        .as_object_mut()
        .unwrap()
        .remove("name");
    assert!(parse_search(&body.to_string(), "tuckface").is_err());
}

#[test]
fn foil_price_used_when_regular_price_is_zero() {
    // Archidekt spells the foil price `ckfoil`.
    let mut body = fixture(SEARCH_FIXTURE);
    body["results"][0]["card"]["prices"] = json!({"ck": 0.0, "ckfoil": 2.5});
    let page = parse_search(&body.to_string(), "tuckface").unwrap();
    assert_eq!(page.cards[0].ck_price, Some(2.5));
}

#[test]
fn missing_prices_object_means_no_price() {
    let mut body = fixture(SEARCH_FIXTURE);
    body["results"][0]["card"]
        .as_object_mut()
        .unwrap()
        .remove("prices");
    let page = parse_search(&body.to_string(), "tuckface").unwrap();
    assert_eq!(page.cards[0].ck_price, None);
}

#[test]
fn full_page_without_paging_metadata_may_be_incomplete() {
    let card = fixture(SEARCH_FIXTURE)["results"][0].clone();
    let page_of = |n: usize| json!({"results": vec![card.clone(); n]}).to_string();

    let full = parse_search(&page_of(ARCHIDEKT_SEARCH_PAGE_SIZE), "u").unwrap();
    assert!(full.may_be_incomplete);
    let partial = parse_search(&page_of(ARCHIDEKT_SEARCH_PAGE_SIZE - 1), "u").unwrap();
    assert!(!partial.may_be_incomplete);
}

#[test]
fn count_equal_to_results_is_complete() {
    let card = fixture(SEARCH_FIXTURE)["results"][0].clone();
    let body = json!({"count": 25, "next": null, "results": vec![card; 25]});
    let page = parse_search(&body.to_string(), "u").unwrap();
    assert!(!page.may_be_incomplete);
}

// ---- deck mapping ----

#[test]
fn deck_fixture_maps_metadata() {
    let deck = parse_deck(DECK_FIXTURE, "3531305").unwrap();
    assert_eq!(deck.title, "The Fighting Dragons");
    assert_eq!(deck.url, "https://archidekt.com/decks/3531305");
    assert_eq!(
        deck.thumbnail.as_deref(),
        Some("https://api.scryfall.com/cards/clb/284?format=image")
    );
    assert_eq!(deck.original_owner, "MakiMakiRoll");
    assert_eq!(deck.last_updated_at, "2024-04-24T16:37:28.613711Z");
}

#[test]
fn deck_without_commander_has_no_thumbnail() {
    let mut body = fixture(DECK_FIXTURE);
    for card in body["cards"].as_array_mut().unwrap() {
        card["categories"] = json!(["Creature"]);
    }
    let deck = parse_deck(&body.to_string(), "3531305").unwrap();
    assert_eq!(deck.title, "The Fighting Dragons");
    assert_eq!(deck.thumbnail, None);
}

#[test]
fn deck_with_flat_shaped_commander() {
    let mut body = fixture(DECK_FIXTURE);
    body["cards"][0]["card"] = json!({"set": "clb", "cn": "284"});
    let deck = parse_deck(&body.to_string(), "1").unwrap();
    assert_eq!(
        deck.thumbnail.as_deref(),
        Some("https://api.scryfall.com/cards/clb/284?format=image")
    );
}

#[test]
fn production_base_url_is_https() {
    assert!(ARCHIDEKT_API_BASE_URL.starts_with("https://"));
}

// ---- HTTP ----

#[tokio::test]
async fn search_sends_card_name_and_maps_response() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/collection/243732/"))
        .and(query_param("cardName", "sol"))
        .respond_with(ResponseTemplate::new(200).set_body_string(SEARCH_FIXTURE))
        .expect(1)
        .mount(&server)
        .await;

    let page = client(&server)
        .search("243732", "sol", "tuckface")
        .await
        .unwrap();
    assert_eq!(page.cards.len(), 4);
    assert_eq!(page.cards[0].owner, "tuckface");
}

#[tokio::test]
async fn search_term_is_query_encoded() {
    let term = "Rock & Roll #1 + Bob's";
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/collection/1/"))
        .and(query_param("cardName", term))
        .respond_with(ResponseTemplate::new(200).set_body_string(r#"{"results":[]}"#))
        .expect(1)
        .mount(&server)
        .await;

    client(&server).search("1", term, "u").await.unwrap();
}

#[tokio::test]
async fn search_404_reports_status() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;

    let err = client(&server).search("1", "sol", "u").await.err().unwrap();
    assert_eq!(
        err.to_string(),
        "Archidekt collection search failed with status code 404 Not Found"
    );
}

#[tokio::test]
async fn search_500_reports_status() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    let err = client(&server).search("1", "sol", "u").await.err().unwrap();
    assert_eq!(
        err.to_string(),
        "Archidekt collection search failed with status code 500 Internal Server Error"
    );
}

#[tokio::test]
async fn search_invalid_json_reports_unexpected_response() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_string("<html>not json</html>"))
        .mount(&server)
        .await;

    let err = client(&server).search("1", "sol", "u").await.err().unwrap();
    assert!(
        err.to_string()
            .starts_with("Archidekt collection search returned an unexpected response"),
        "{err}"
    );
}

#[tokio::test]
async fn get_deck_maps_response() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/decks/3531305/"))
        .respond_with(ResponseTemplate::new(200).set_body_string(DECK_FIXTURE))
        .expect(1)
        .mount(&server)
        .await;

    let deck = client(&server).get_deck("3531305").await.unwrap();
    assert_eq!(deck.title, "The Fighting Dragons");
    assert_eq!(deck.url, "https://archidekt.com/decks/3531305");
}

#[tokio::test]
async fn get_deck_404_reports_status() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;

    let err = client(&server).get_deck("1").await.err().unwrap();
    assert_eq!(
        err.to_string(),
        "Archidekt deck lookup failed with status code 404 Not Found"
    );
}

#[tokio::test]
async fn get_deck_invalid_json_reports_unexpected_response() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_string("{\"name\": 1"))
        .mount(&server)
        .await;

    let err = client(&server).get_deck("1").await.err().unwrap();
    assert!(
        err.to_string()
            .starts_with("Archidekt deck lookup returned an unexpected response"),
        "{err}"
    );
}
