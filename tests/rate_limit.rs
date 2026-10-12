//! Provider pacing for collection search and community decks, against a local wiremock
//! server, under paused tokio time.
//!
//! Paused time normally auto-advances to the next timer whenever the runtime is idle,
//! including while a request waits on the mock server's socket, which would fire reqwest's
//! 20s request timeout. [`FrozenClock`] stops that (tokio does not auto-advance while a
//! `spawn_blocking` task runs), so the clock only moves when a test calls `advance`. Every
//! request therefore goes out at a known virtual time, and nothing sleeps for real.

use fb3k_discord_bot::models::config::{
    MTGCollectionConfig, MTGCollectionProvider, MTGCommunityDeck,
};
use fb3k_discord_bot::mtg::community_decks::fetch_community_decks;
use fb3k_discord_bot::mtg::providers::{ArchidektClient, MoxfieldClient, ProviderClients};
use fb3k_discord_bot::mtg::search::fetch_collection_searches;
use std::future::Future;
use std::sync::mpsc;
use tokio::sync::watch;
use tokio::time::{advance, Duration, Instant};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

use MTGCollectionProvider::{Archidekt, Moxfield};

const MOXFIELD_DECK: &str = include_str!("fixtures/moxfield_deck.json");
const ARCHIDEKT_DECK: &str = include_str!("fixtures/archidekt_deck.json");

const SECOND: Duration = Duration::from_secs(1);
const MILLI: Duration = Duration::from_millis(1);
/// Real time after which a frozen clock thaws, so a broken test fails instead of hanging.
const WATCHDOG: std::time::Duration = std::time::Duration::from_secs(10);

/// While alive, paused time does not auto-advance; it moves only on `advance`.
struct FrozenClock(#[allow(dead_code)] mpsc::Sender<()>);

impl FrozenClock {
    fn new() -> Self {
        let (tx, rx) = mpsc::channel::<()>();
        // Returns as soon as the sender is dropped (or after the watchdog).
        tokio::task::spawn_blocking(move || {
            let _ = rx.recv_timeout(WATCHDOG);
        });
        Self(tx)
    }
}

/// Answers every provider endpoint with an empty/fixture body and publishes the path of
/// each request as it arrives. Paths containing `fail` get a 500.
struct Recorder(watch::Sender<Vec<String>>);

impl Respond for Recorder {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let path = request.url.path().to_string();
        self.0.send_modify(|paths| paths.push(path.clone()));
        if path.contains("fail") {
            return ResponseTemplate::new(500);
        }
        let body = if path.starts_with("/v1/trade-binders/") {
            r#"{"data":[],"totalResults":0}"#
        } else if path.starts_with("/collection/") {
            r#"{"results":[],"count":0}"#
        } else if path.starts_with("/v3/decks/") {
            MOXFIELD_DECK
        } else if path.starts_with("/decks/") {
            ARCHIDEKT_DECK
        } else {
            return ResponseTemplate::new(404);
        };
        ResponseTemplate::new(200).set_body_string(body)
    }
}

struct Harness {
    _server: MockServer,
    requests: watch::Receiver<Vec<String>>,
    providers: Option<ProviderClients>,
}

impl Harness {
    async fn start() -> Self {
        let server = MockServer::start().await;
        let (tx, requests) = watch::channel(Vec::new());
        Mock::given(wiremock::matchers::any())
            .respond_with(Recorder(tx))
            .mount(&server)
            .await;
        let providers = ProviderClients {
            archidekt: ArchidektClient::with_base_url(server.uri()).unwrap(),
            moxfield: MoxfieldClient::with_base_url(server.uri(), "fb3k-test-agent/1.0").unwrap(),
        };
        Self {
            _server: server,
            requests,
            providers: Some(providers),
        }
    }

    /// Wait until `n` requests have arrived in total; returns the requests that arrived
    /// since the previous `n` passed in (sorted, since concurrent arrivals race).
    async fn arrivals(&mut self, seen: usize, n: usize) -> Vec<String> {
        let paths = self.requests.wait_for(|p| p.len() >= n).await.unwrap();
        let mut new: Vec<String> = paths[seen..].to_vec();
        new.sort();
        new
    }

    fn total_requests(&self) -> usize {
        self.requests.borrow().len()
    }
}

fn collection(provider: MTGCollectionProvider, id: &str) -> MTGCollectionConfig {
    MTGCollectionConfig {
        provider,
        discord_user: format!("owner-of-{id}"),
        provider_collection: id.to_string(),
    }
}

fn community_deck(provider: MTGCollectionProvider, id: &str) -> MTGCommunityDeck {
    MTGCommunityDeck {
        provider,
        discord_user: "someone".to_string(),
        provider_deck: id.to_string(),
    }
}

fn search_path(provider: MTGCollectionProvider, id: &str) -> String {
    match provider {
        Archidekt => format!("/collection/{id}/"),
        Moxfield => format!("/v1/trade-binders/{id}/search"),
    }
}

fn deck_path(provider: MTGCollectionProvider, id: &str) -> String {
    match provider {
        Archidekt => format!("/decks/{id}/"),
        Moxfield => format!("/v3/decks/all/{id}"),
    }
}

fn sorted(mut v: Vec<String>) -> Vec<String> {
    v.sort();
    v
}

/// Run `work` as its own task; it reports how long after `start` (virtual) it finished.
fn spawn_timed<T: Send + 'static>(
    start: Instant,
    work: impl Future<Output = T> + Send + 'static,
) -> tokio::task::JoinHandle<(T, Duration)> {
    tokio::spawn(async move {
        let out = work.await;
        (out, start.elapsed())
    })
}

#[tokio::test(start_paused = true)]
async fn search_paces_moxfield_but_not_archidekt() {
    let mut h = Harness::start().await;
    let providers = h.providers.take().unwrap();
    let collections = vec![
        collection(Moxfield, "mox-a"),
        collection(Archidekt, "arch-1"),
        collection(Moxfield, "mox-fail-b"),
        collection(Archidekt, "arch-2"),
        collection(Moxfield, "mox-c"),
    ];

    let _frozen = FrozenClock::new();
    let start = Instant::now();
    let task = spawn_timed(start, async move {
        fetch_collection_searches("sol", &collections, &providers).await
    });

    // t=0: the first Moxfield call and every Archidekt call go out at once.
    assert_eq!(
        h.arrivals(0, 3).await,
        sorted(vec![
            search_path(Moxfield, "mox-a"),
            search_path(Archidekt, "arch-1"),
            search_path(Archidekt, "arch-2"),
        ])
    );

    // Just short of 1s: still held back.
    advance(SECOND - MILLI).await;
    tokio::task::yield_now().await;
    assert_eq!(h.total_requests(), 3);

    // t=1s: the next Moxfield call, in config order (a failed call still counts).
    advance(MILLI).await;
    assert_eq!(
        h.arrivals(3, 4).await,
        vec![search_path(Moxfield, "mox-fail-b")]
    );

    // t=2s: the last one.
    advance(SECOND).await;
    assert_eq!(h.arrivals(4, 5).await, vec![search_path(Moxfield, "mox-c")]);

    // Done at t=2s exactly: no wait after the last request.
    let (searches, finished_at) = task.await.unwrap();
    assert_eq!(finished_at, 2 * SECOND);

    // One outcome per collection, in config order.
    let owners: Vec<&str> = searches.iter().map(|s| s.discord_user.as_str()).collect();
    assert_eq!(
        owners,
        vec![
            "owner-of-mox-a",
            "owner-of-arch-1",
            "owner-of-mox-fail-b",
            "owner-of-arch-2",
            "owner-of-mox-c"
        ]
    );
    let ok: Vec<bool> = searches.iter().map(|s| s.result.is_ok()).collect();
    assert_eq!(ok, vec![true, true, false, true, true]);
}

#[tokio::test(start_paused = true)]
async fn archidekt_only_search_is_not_delayed() {
    let mut h = Harness::start().await;
    let providers = h.providers.take().unwrap();
    let collections: Vec<_> = (0..4)
        .map(|i| collection(Archidekt, &format!("arch-{i}")))
        .collect();

    let _frozen = FrozenClock::new();
    let start = Instant::now();
    let (searches, finished_at) = spawn_timed(start, async move {
        fetch_collection_searches("sol", &collections, &providers).await
    })
    .await
    .unwrap();

    assert_eq!(finished_at, Duration::ZERO);
    assert!(searches.iter().all(|s| s.result.is_ok()));
    assert_eq!(h.total_requests(), 4);
}

#[tokio::test(start_paused = true)]
async fn single_moxfield_search_has_no_trailing_wait() {
    let mut h = Harness::start().await;
    let providers = h.providers.take().unwrap();
    let collections = vec![collection(Moxfield, "mox-only")];

    let _frozen = FrozenClock::new();
    let start = Instant::now();
    let (searches, finished_at) = spawn_timed(start, async move {
        fetch_collection_searches("sol", &collections, &providers).await
    })
    .await
    .unwrap();

    assert_eq!(finished_at, Duration::ZERO);
    assert!(searches[0].result.is_ok());
}

#[tokio::test(start_paused = true)]
async fn community_decks_pace_moxfield_but_not_archidekt() {
    let mut h = Harness::start().await;
    let providers = h.providers.take().unwrap();
    let decks = vec![
        community_deck(Moxfield, "deck-mox-a"),
        community_deck(Moxfield, "deck-mox-b"),
        community_deck(Archidekt, "deck-arch"),
    ];

    let _frozen = FrozenClock::new();
    let start = Instant::now();
    let task = spawn_timed(start, async move {
        fetch_community_decks(&decks, &providers).await
    });

    assert_eq!(
        h.arrivals(0, 2).await,
        sorted(vec![
            deck_path(Moxfield, "deck-mox-a"),
            deck_path(Archidekt, "deck-arch"),
        ])
    );

    advance(SECOND - MILLI).await;
    tokio::task::yield_now().await;
    assert_eq!(h.total_requests(), 2);

    advance(MILLI).await;
    assert_eq!(
        h.arrivals(2, 3).await,
        vec![deck_path(Moxfield, "deck-mox-b")]
    );

    let (lookups, finished_at) = task.await.unwrap();
    assert_eq!(finished_at, SECOND);
    let ids: Vec<&str> = lookups.iter().map(|l| l.deck_id.as_str()).collect();
    assert_eq!(ids, vec!["deck-mox-a", "deck-mox-b", "deck-arch"]);
    assert!(lookups.iter().all(|l| l.result.is_ok()));
}

#[tokio::test(start_paused = true)]
async fn search_and_community_decks_share_one_moxfield_limiter() {
    let mut h = Harness::start().await;
    let providers = h.providers.take().unwrap();
    let collections = vec![
        collection(Moxfield, "mox-search"),
        collection(Archidekt, "arch-search"),
    ];
    let decks = vec![
        community_deck(Moxfield, "deck-mox"),
        community_deck(Archidekt, "deck-arch"),
    ];

    // A search and a deck listing at the same time (e.g. two users' commands).
    let _frozen = FrozenClock::new();
    let start = Instant::now();
    let task = spawn_timed(start, async move {
        tokio::join!(
            fetch_collection_searches("sol", &collections, &providers),
            fetch_community_decks(&decks, &providers),
        )
    });

    // t=0: only one Moxfield request between them, plus both Archidekt ones.
    assert_eq!(
        h.arrivals(0, 3).await,
        sorted(vec![
            search_path(Moxfield, "mox-search"),
            search_path(Archidekt, "arch-search"),
            deck_path(Archidekt, "deck-arch"),
        ])
    );

    // The deck lookup waits on the same limiter the search used.
    advance(SECOND - MILLI).await;
    tokio::task::yield_now().await;
    assert_eq!(h.total_requests(), 3);

    advance(MILLI).await;
    assert_eq!(
        h.arrivals(3, 4).await,
        vec![deck_path(Moxfield, "deck-mox")]
    );

    let ((searches, lookups), finished_at) = task.await.unwrap();
    assert_eq!(finished_at, SECOND);
    assert!(searches.iter().all(|s| s.result.is_ok()));
    assert!(lookups.iter().all(|l| l.result.is_ok()));
}
