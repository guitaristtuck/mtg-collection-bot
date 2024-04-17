mod commands;
mod mtg;
mod models;
mod interactions;

use std::env;

use serenity::async_trait;
use serenity::builder::EditInteractionResponse;
use serenity::model::application::Interaction;
use serenity::model::gateway::Ready;
use serenity::Error as SerenityError;
use serenity::model::id::GuildId;
use serenity::model::voice::VoiceState; 
use serenity::prelude::*;
use dotenv::dotenv;
use models::config::{BotConfig,load_config};

use log;

struct Handler {
    config: BotConfig
}

#[async_trait]
impl EventHandler for Handler {
    // main handler for processing commands
    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        if let Interaction::Command(command) = interaction {
            log::info!("Received command interaction: {command:#?}");

            //defer the response to allow for slow commands
            command.defer(&ctx.http).await.unwrap();

            let response = match command.data.name.as_str() {
                "ping" => Some(commands::ping::run(&command.data.options())),
                "mtg" => Some(commands::mtg::run(&command.data.options(),&self.config).await),
                _ => Some(EditInteractionResponse::new().content("Command not implemented :(")),
            };

            if let Some(response) = response {
                if let Err(why) = command.edit_response(&ctx.http, response).await {
                    match why {
                        SerenityError::Model(e) => log::error!("Error sending command response: Model Error: {}", e),
                        SerenityError::Http(e) => log::error!("Error sending command response: Http Error: {}", e),
                        SerenityError::Json(e) => log::error!("Error sending command response: Json Error: {}", e),
                        _ => log::error!("Error sending command response: Unknown Error: {}", why),
                    }
                }
            }
           
        }
    }

    // interact with channel voice changes
    async fn voice_state_update(&self, ctx: Context, old: Option<VoiceState>, new: VoiceState) {
        log::info!("voice join detected");
        interactions::nubby::voice_state_update(ctx, &self.config, old, new).await;
    }

    // set up commands on ready
    async fn ready(&self, ctx: Context, ready: Ready) {
        log::info!("{} is connected!", ready.user.name);

        let guild_id = GuildId::new(
            env::var("GUILD_ID")
                .expect("Expected GUILD_ID in environment")
                .parse()
                .expect("GUILD_ID must be an integer"),
        );

        let commands = guild_id
            .set_commands(&ctx.http, vec![
                commands::ping::register(),
                commands::mtg::register()
            ])
            .await;

        log::info!("I now have the following guild slash commands: {commands:#?}");
    }
}

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
    let mut client = Client::builder(token, 
        GatewayIntents::GUILDS
        | GatewayIntents::GUILD_VOICE_STATES
    )
    .event_handler(
        Handler {
            config: load_config()
        }
    )
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