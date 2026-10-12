use std::env;

use dotenv::dotenv;
use fb3k_discord_bot::models::config::load_config;
use fb3k_discord_bot::Handler;
use serenity::prelude::*;

#[tokio::main]
async fn main() {
    // set up logging
    log4rs::init_file("log4rs.yaml", Default::default()).unwrap();
    log::info!("logging configured");

    // load dotenv
    dotenv().ok();

    // Login with a bot token from the environment
    let token = env::var("DISCORD_TOKEN").expect("Expected a token in the environment");

    // Build our client.
    let mut client = Client::builder(
        token,
        GatewayIntents::GUILDS | GatewayIntents::GUILD_VOICE_STATES,
    )
    .event_handler(Handler {
        config: load_config(),
    })
    .await
    .expect("Error creating client");

    // Finally, start a single shard, and start listening to events.
    //
    // Shards will automatically attempt to reconnect, and will perform exponential backoff until
    // it reconnects.
    if let Err(why) = client.start().await {
        log::info!("Client error: {why:?}");
    }
}
