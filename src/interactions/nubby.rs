use serenity::all::{Context,VoiceState,UserId,ChannelId,CreateMessage,ReactionType};
use crate::models::config::BotConfig;
use log;
use chrono::{Utc,Duration};


pub const NUBBY_ANNOUNCE_RESET_HOURS: i64 = 12;

pub async fn voice_state_update(ctx: Context, config: &BotConfig, _old: Option<VoiceState>, new: VoiceState) {
    // It's nubby time
    let nubby = UserId::new(config.nubby.nubby_user_id); // Nubby's user 
    let text_channel = ChannelId::new(config.common.general_channel_id); // General text channel
    let mut announce_nubby = false;

    // get the last time nubby was announced
    {
        if new.user_id == nubby && new.channel_id.is_some() {
            let mut last_announced = config.nubby.nubby_last_announced
            .lock()
            .unwrap();

            if *last_announced + Duration::hours(NUBBY_ANNOUNCE_RESET_HOURS) < Utc::now() {
                *last_announced = Utc::now();
                announce_nubby = true;
            } else {
                log::info!("Nubby detected, but he was announced within the last {} hours at {}",NUBBY_ANNOUNCE_RESET_HOURS,last_announced)
            }
        }
    }
  
    if announce_nubby {
        log::info!("It's nubby time!");
        match text_channel.send_message(&ctx.http, 
            CreateMessage::new()
                .content("https://tenor.com/view/nubby-noddy-nubby-noddy-gif-26762228")
                .reactions(
                    vec![
                        ReactionType::Unicode("🇳".into()),
                        ReactionType::Unicode("🇺".into()),
                        ReactionType::Unicode("🇧".into()),
                        ReactionType::Unicode("🅱️".into()),
                        ReactionType::Unicode("🇾".into())
                    ]
                )
        ).await {
            Ok(_) => (),
            Err(e) => log::error!("Could not announce nubby. error: {}", e)
        }
    }
}