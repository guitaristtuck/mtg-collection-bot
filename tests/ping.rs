use fb3k_discord_bot::commands::ping;

#[test]
fn ping_responds_with_ready_message() {
    let response = serde_json::to_value(ping::run(&[])).unwrap();

    assert_eq!(
        response["content"],
        "F3BK bot reporting in, ready for nonsense"
    );
}
