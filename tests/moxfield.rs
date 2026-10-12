//! Moxfield provider: fixture mapping tests and HTTP tests against a local wiremock server.

use fb3k_discord_bot::mtg::providers::moxfield::{
    parse_deck, parse_search, phrase_query, MoxfieldClient, MOXFIELD_API_BASE_URL,
    MOXFIELD_SEARCH_PAGE_SIZE,
};
use serde_json::{json, Value};
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const SEARCH_FIXTURE: &str = include_str!("fixtures/moxfield_search.json");
const DECK_FIXTURE: &str = include_str!("fixtures/moxfield_deck.json");
const TEST_UA: &str = "fb3k-test-agent/1.0";

fn fixture(json: &str) -> Value {
    serde_json::from_str(json).unwrap()
}

fn client(server: &MockServer) -> MoxfieldClient {
    MoxfieldClient::with_base_url(server.uri(), TEST_UA).unwrap()
}

// ---- search mapping ----

#[test]
fn search_fixture_maps_cards() {
    let page = parse_search(SEARCH_FIXTURE, ".sparky").unwrap();
    assert_eq!(page.cards.len(), 3);

    let sorin = &page.cards[0];
    assert_eq!(sorin.name, "Sorin, Solemn Visitor");
    assert_eq!((sorin.set.as_str(), sorin.cn.as_str()), ("ktk", "202"));
    assert_eq!(sorin.quantity, 1);
    assert_eq!(sorin.owner, ".sparky");
    assert_eq!(sorin.ck_price, Some(1.29));

    // ck_foil is null for this printing
    assert_eq!(page.cards[1].name, "Solemn Simulacrum");
    assert_eq!(page.cards[1].ck_price, Some(0.49));
}

#[test]
fn search_fixture_maps_double_faced_card() {
    let page = parse_search(SEARCH_FIXTURE, ".sparky").unwrap();
    let dfc = &page.cards[2];
    assert_eq!(dfc.name, "Volatile Arsonist // Dire-Strain Anarchist");
    assert_eq!((dfc.set.as_str(), dfc.cn.as_str()), ("vow", "181"));
}

#[test]
fn search_fixture_with_more_total_results_may_be_incomplete() {
    // totalResults 45 > the cards returned
    let page = parse_search(SEARCH_FIXTURE, ".sparky").unwrap();
    assert!(page.may_be_incomplete);
}

#[test]
fn total_results_equal_to_returned_is_complete() {
    let mut body = fixture(SEARCH_FIXTURE);
    body["totalResults"] = json!(3);
    body["totalPages"] = json!(1);
    let page = parse_search(&body.to_string(), "u").unwrap();
    assert!(!page.may_be_incomplete);
}

#[test]
fn full_page_without_total_may_be_incomplete() {
    let card = fixture(SEARCH_FIXTURE)["data"][0].clone();
    let page_of = |n: usize| json!({"data": vec![card.clone(); n]}).to_string();

    assert!(
        parse_search(&page_of(MOXFIELD_SEARCH_PAGE_SIZE), "u")
            .unwrap()
            .may_be_incomplete
    );
    assert!(
        !parse_search(&page_of(MOXFIELD_SEARCH_PAGE_SIZE - 1), "u")
            .unwrap()
            .may_be_incomplete
    );
}

#[test]
fn empty_search_results() {
    let body = json!({"pageNumber": 1, "pageSize": 25, "totalResults": 0, "totalPages": 0, "hasMore": false, "data": []});
    let page = parse_search(&body.to_string(), "u").unwrap();
    assert!(page.cards.is_empty());
    assert!(!page.may_be_incomplete);
}

#[test]
fn unknown_extra_fields_are_ignored() {
    let mut body = fixture(SEARCH_FIXTURE);
    body["brandNew"] = json!([{"a": 1}]);
    body["data"][0]["card"]["somethingElse"] = json!(null);
    let page = parse_search(&body.to_string(), "u").unwrap();
    assert_eq!(page.cards.len(), 3);
}

#[test]
fn malformed_card_fails_the_whole_search() {
    let mut body = fixture(SEARCH_FIXTURE);
    body["data"][2]["card"]["cn"] = json!(181);
    assert!(parse_search(&body.to_string(), "u").is_err());
}

#[test]
fn price_falls_back_to_foil_then_etched() {
    let mut body = fixture(SEARCH_FIXTURE);
    body["data"][0]["card"]["prices"] = json!({"ck": null, "ck_foil": 5.99, "ck_etched": 9.0});
    body["data"][1]["card"]["prices"] = json!({"ck": 0, "ck_foil": 0, "ck_etched": 9.0});
    body["data"][2]["card"]["prices"] = json!({"ck": null, "ck_foil": null, "ck_etched": null});
    let page = parse_search(&body.to_string(), "u").unwrap();
    assert_eq!(page.cards[0].ck_price, Some(5.99));
    assert_eq!(page.cards[1].ck_price, Some(9.0));
    assert_eq!(page.cards[2].ck_price, None);
}

// ---- deck mapping ----

#[test]
fn deck_fixture_maps_metadata() {
    let deck = parse_deck(DECK_FIXTURE).unwrap();
    assert_eq!(deck.title, "Kenrith");
    assert_eq!(
        deck.url,
        "https://moxfield.com/decks/ev681gZZkEOhPGQ9IqoHWQ"
    );
    assert_eq!(
        deck.thumbnail.as_deref(),
        Some("https://api.scryfall.com/cards/plst/ELD-303?format=image")
    );
    assert_eq!(deck.original_owner, "scondo62");
    assert_eq!(deck.last_updated_at, "2024-04-07T14:24:25.713Z");
}

#[test]
fn deck_without_main_card_has_no_thumbnail() {
    let mut body = fixture(DECK_FIXTURE);
    body.as_object_mut().unwrap().remove("main");
    let deck = parse_deck(&body.to_string()).unwrap();
    assert_eq!(deck.thumbnail, None);
    assert_eq!(deck.title, "Kenrith");
}

// ---- query building ----

#[test]
fn search_term_is_wrapped_in_quotes() {
    assert_eq!(phrase_query("sol ring"), "\"sol ring\"");
}

#[test]
fn production_base_url_is_https() {
    assert!(MOXFIELD_API_BASE_URL.starts_with("https://"));
}

// ---- HTTP ----

#[tokio::test]
async fn search_sends_phrase_query_and_maps_response() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/trade-binders/PUYRTn5Q2kqiBB531_1W1Q/search"))
        .and(query_param("q", "\"sol\""))
        .respond_with(ResponseTemplate::new(200).set_body_string(SEARCH_FIXTURE))
        .expect(1)
        .mount(&server)
        .await;

    let page = client(&server)
        .search("PUYRTn5Q2kqiBB531_1W1Q", "sol", ".sparky")
        .await
        .unwrap();
    assert_eq!(page.cards.len(), 3);
}

#[tokio::test]
async fn special_characters_in_search_term_round_trip() {
    for term in [
        "Rock & Roll",
        "Ashiok #1",
        "+2 Mace",
        "Urza's Saga",
        "Fire // Ice",
        "a&b#c+d'e f",
    ] {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/trade-binders/b/search"))
            .and(query_param("q", format!("\"{term}\"")))
            .respond_with(ResponseTemplate::new(200).set_body_string(r#"{"data":[]}"#))
            .expect(1)
            .mount(&server)
            .await;

        client(&server).search("b", term, "u").await.unwrap();

        let requests = server.received_requests().await.unwrap();
        let raw_query = requests[0].url.query().unwrap_or_default().to_string();
        assert!(!raw_query.contains('#'), "{term}: {raw_query}");
        assert_eq!(
            raw_query.matches('&').count(),
            0,
            "{term}: '&' must be escaped in {raw_query}"
        );
    }
}

#[tokio::test]
async fn user_agent_and_accept_sent_on_every_request() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/trade-binders/b/search"))
        .respond_with(ResponseTemplate::new(200).set_body_string(SEARCH_FIXTURE))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v3/decks/all/d"))
        .respond_with(ResponseTemplate::new(200).set_body_string(DECK_FIXTURE))
        .mount(&server)
        .await;

    let client = client(&server);
    client.search("b", "sol", "u").await.unwrap();
    client.get_deck("d").await.unwrap();
    client.search("b", "ring", "u").await.unwrap();

    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 3);
    for request in &requests {
        let ua = request
            .headers
            .get("user-agent")
            .map(|v| v.to_str().unwrap());
        assert_eq!(ua, Some(TEST_UA), "{}", request.url);
        let accept = request.headers.get("accept").map(|v| v.to_str().unwrap());
        assert_eq!(accept, Some("application/json"), "{}", request.url);
        assert!(
            request.headers.get("content-type").is_none(),
            "{}",
            request.url
        );
    }
}

#[tokio::test]
async fn request_without_user_agent_does_not_match() {
    // Guard against the header matcher passing vacuously.
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(header("user-agent", TEST_UA))
        .respond_with(ResponseTemplate::new(200).set_body_string(DECK_FIXTURE))
        .mount(&server)
        .await;

    let other = MoxfieldClient::with_base_url(server.uri(), "someone-else").unwrap();
    let err = other.get_deck("d").await.err().unwrap();
    assert!(err.to_string().contains("404"), "{err}");
    client(&server).get_deck("d").await.unwrap();
}

#[tokio::test]
async fn search_404_reports_status() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;

    let err = client(&server).search("b", "sol", "u").await.err().unwrap();
    assert_eq!(
        err.to_string(),
        "Moxfield collection search failed with status code 404 Not Found"
    );
}

#[tokio::test]
async fn search_500_reports_status() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    let err = client(&server).search("b", "sol", "u").await.err().unwrap();
    assert_eq!(
        err.to_string(),
        "Moxfield collection search failed with status code 500 Internal Server Error"
    );
}

#[tokio::test]
async fn search_invalid_json_reports_unexpected_response() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_string("HACK THE PLANET!"))
        .mount(&server)
        .await;

    let err = client(&server).search("b", "sol", "u").await.err().unwrap();
    assert!(
        err.to_string()
            .starts_with("Moxfield collection search returned an unexpected response"),
        "{err}"
    );
}

#[tokio::test]
async fn get_deck_403_reports_status() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(403))
        .mount(&server)
        .await;

    let err = client(&server).get_deck("d").await.err().unwrap();
    assert_eq!(
        err.to_string(),
        "Moxfield deck lookup failed with status code 403 Forbidden"
    );
}

#[tokio::test]
async fn get_deck_500_reports_status() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    let err = client(&server).get_deck("d").await.err().unwrap();
    assert_eq!(
        err.to_string(),
        "Moxfield deck lookup failed with status code 500 Internal Server Error"
    );
}

#[tokio::test]
async fn get_deck_invalid_json_reports_unexpected_response() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_string("[]"))
        .mount(&server)
        .await;

    let err = client(&server).get_deck("d").await.err().unwrap();
    assert!(
        err.to_string()
            .starts_with("Moxfield deck lookup returned an unexpected response"),
        "{err}"
    );
}
