use crate::commands;
use crate::interactions;
use crate::models::config::BotConfig;
use serenity::async_trait;
use serenity::builder::EditInteractionResponse;
use serenity::model::application::Interaction;
use serenity::model::gateway::Ready;
use serenity::model::id::GuildId;
use serenity::model::voice::VoiceState;
use serenity::prelude::*;
use serenity::Error as SerenityError;

pub struct Handler {
    pub config: BotConfig,
}

#[async_trait]
impl EventHandler for Handler {
    // main handler for processing commands
    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        if let Interaction::Command(command) = interaction {
            log::info!("Received command interaction: {command:#?}");

            //defer the response to allow for slow commands
            if let Err(why) = command.defer(&ctx.http).await {
                log::error!("Error deferring command response: {why}");
                return;
            }

            let response = match command.data.name.as_str() {
                "ping" => Some(commands::ping::run(&command.data.options())),
                "mtg" => Some(commands::mtg::run(&command.data.options(), &self.config).await),
                _ => Some(EditInteractionResponse::new().content("Command not implemented :(")),
            };

            if let Some(response) = response {
                if let Err(why) = command.edit_response(&ctx.http, response).await {
                    match why {
                        SerenityError::Model(e) => {
                            log::error!("Error sending command response: Model Error: {}", e)
                        }
                        SerenityError::Http(e) => {
                            log::error!("Error sending command response: Http Error: {}", e)
                        }
                        SerenityError::Json(e) => {
                            log::error!("Error sending command response: Json Error: {}", e)
                        }
                        _ => log::error!("Error sending command response: Unknown Error: {}", why),
                    }
                }
            }
        }
    }

    // interact with channel voice changes
    async fn voice_state_update(&self, ctx: Context, old: Option<VoiceState>, new: VoiceState) {
        interactions::nubby::voice_state_update(ctx, &self.config, old, new).await;
    }

    // set up commands on ready
    async fn ready(&self, ctx: Context, ready: Ready) {
        log::info!("{} is connected!", ready.user.name);

        let guild_id = GuildId::new(self.config.env.guild_id);

        let commands = guild_id
            .set_commands(
                &ctx.http,
                vec![commands::ping::register(), commands::mtg::register()],
            )
            .await;

        log::info!("I now have the following guild slash commands: {commands:#?}");
    }
}
