use crate::models::config::BotConfig;
use chrono::{DateTime, Duration, Utc};
use log;
use serenity::all::{ChannelId, Context, CreateMessage, GuildId, ReactionType, UserId, VoiceState};

pub const NUBBY_ANNOUNCE_RESET_HOURS: i64 = 12;

/// Who and where we are watching for Nubby.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NubbyWatch {
    /// The only guild whose voice events count (`GUILD_ID`).
    pub guild_id: GuildId,
    /// Nubby's Discord user.
    pub nubby_user_id: UserId,
}

/// Decide whether this voice state change should trigger the Nubby announcement.
///
/// Pure: no I/O and no clock. Announces only when
/// - the event is for `watch.guild_id`,
/// - the user is `watch.nubby_user_id`,
/// - it is a real join: `old` was in no voice channel and `new` is in one
///   (mute/deafen/stream/video toggles and channel moves have `old` in a channel),
/// - more than [`NUBBY_ANNOUNCE_RESET_HOURS`] have passed since `*last_announced`.
///
/// On `true`, `*last_announced` is set to `now`; otherwise it is left unchanged.
///
/// `old` is serenity's cached previous voice state: `None` when the user was not
/// in a voice channel (the cache only keeps states that have a channel).
pub fn should_announce(
    old: Option<&VoiceState>,
    new: &VoiceState,
    watch: NubbyWatch,
    last_announced: &mut DateTime<Utc>,
    now: DateTime<Utc>,
) -> bool {
    let was_in_voice = old.is_some_and(|o| o.channel_id.is_some());
    let is_real_join = !was_in_voice && new.channel_id.is_some();

    if new.guild_id != Some(watch.guild_id) || new.user_id != watch.nubby_user_id || !is_real_join {
        return false;
    }

    if now - *last_announced <= Duration::hours(NUBBY_ANNOUNCE_RESET_HOURS) {
        log::info!(
            "Nubby joined, but he was announced within the last {} hours at {}",
            NUBBY_ANNOUNCE_RESET_HOURS,
            last_announced
        );
        return false;
    }

    *last_announced = now;
    true
}

pub async fn voice_state_update(
    ctx: Context,
    config: &BotConfig,
    old: Option<VoiceState>,
    new: VoiceState,
) {
    let watch = NubbyWatch {
        guild_id: GuildId::new(config.env.guild_id),
        nubby_user_id: UserId::new(config.nubby.nubby_user_id),
    };

    let announce = {
        let mut last_announced = config.nubby.nubby_last_announced.lock().unwrap();
        should_announce(old.as_ref(), &new, watch, &mut last_announced, Utc::now())
    };

    if !announce {
        return;
    }

    log::info!("It's nubby time!");
    let text_channel = ChannelId::new(config.common.general_channel_id);
    if let Err(e) = text_channel
        .send_message(
            &ctx.http,
            CreateMessage::new()
                .content("https://tenor.com/view/nubby-noddy-nubby-noddy-gif-26762228")
                .reactions(vec![
                    ReactionType::Unicode("🇳".into()),
                    ReactionType::Unicode("🇺".into()),
                    ReactionType::Unicode("🇧".into()),
                    ReactionType::Unicode("🅱️".into()),
                    ReactionType::Unicode("🇾".into()),
                ]),
        )
        .await
    {
        log::error!("Could not announce nubby. error: {}", e);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use serde_json::json;

    const GUILD: u64 = 1223050445128466553;
    const OTHER_GUILD: u64 = 999;
    const NUBBY: u64 = 115656014824603651;
    const SOMEONE_ELSE: u64 = 42;
    const CHANNEL_A: u64 = 1001;
    const CHANNEL_B: u64 = 1002;

    fn watch() -> NubbyWatch {
        NubbyWatch {
            guild_id: GuildId::new(GUILD),
            nubby_user_id: UserId::new(NUBBY),
        }
    }

    fn state(guild: u64, user: u64, channel: Option<u64>, self_mute: bool) -> VoiceState {
        serde_json::from_value(json!({
            "guild_id": guild.to_string(),
            "channel_id": channel.map(|c| c.to_string()),
            "user_id": user.to_string(),
            "session_id": "s",
            "deaf": false,
            "mute": false,
            "self_deaf": false,
            "self_mute": self_mute,
            "self_video": false,
            "suppress": false,
            "request_to_speak_timestamp": null,
        }))
        .unwrap()
    }

    fn in_channel(user: u64, channel: u64) -> VoiceState {
        state(GUILD, user, Some(channel), false)
    }

    fn left(user: u64) -> VoiceState {
        state(GUILD, user, None, false)
    }

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 12, 20, 0, 0).unwrap()
    }

    /// Like the startup default: last announced 13h ago.
    fn startup_last_announced() -> DateTime<Utc> {
        now() - Duration::hours(NUBBY_ANNOUNCE_RESET_HOURS + 1)
    }

    fn decide(
        old: Option<&VoiceState>,
        new: &VoiceState,
        last: DateTime<Utc>,
    ) -> (bool, DateTime<Utc>) {
        let mut last = last;
        let announced = should_announce(old, new, watch(), &mut last, now());
        (announced, last)
    }

    #[test]
    fn first_join_after_startup_announces_and_records_now() {
        let (announced, last) = decide(
            None,
            &in_channel(NUBBY, CHANNEL_A),
            startup_last_announced(),
        );
        assert!(announced);
        assert_eq!(last, now());
    }

    #[test]
    fn different_user_joining_is_ignored() {
        let start = startup_last_announced();
        let (announced, last) = decide(None, &in_channel(SOMEONE_ELSE, CHANNEL_A), start);
        assert!(!announced);
        assert_eq!(last, start);
    }

    #[test]
    fn leaving_a_channel_is_ignored() {
        let start = startup_last_announced();
        let old = in_channel(NUBBY, CHANNEL_A);
        let (announced, last) = decide(Some(&old), &left(NUBBY), start);
        assert!(!announced);
        assert_eq!(last, start);
    }

    #[test]
    fn leave_event_without_cached_old_state_is_ignored() {
        let (announced, _) = decide(None, &left(NUBBY), startup_last_announced());
        assert!(!announced);
    }

    #[test]
    fn mute_while_in_channel_is_ignored() {
        let old = in_channel(NUBBY, CHANNEL_A);
        let new = state(GUILD, NUBBY, Some(CHANNEL_A), true);
        let (announced, _) = decide(Some(&old), &new, startup_last_announced());
        assert!(!announced);
    }

    #[test]
    fn deafen_while_in_channel_is_ignored() {
        let old = in_channel(NUBBY, CHANNEL_A);
        let mut new = in_channel(NUBBY, CHANNEL_A);
        new.self_deaf = true;
        let (announced, _) = decide(Some(&old), &new, startup_last_announced());
        assert!(!announced);
    }

    #[test]
    fn moving_between_channels_is_ignored() {
        let old = in_channel(NUBBY, CHANNEL_A);
        let new = in_channel(NUBBY, CHANNEL_B);
        let (announced, _) = decide(Some(&old), &new, startup_last_announced());
        assert!(!announced);
    }

    #[test]
    fn join_in_another_guild_is_ignored() {
        let new = state(OTHER_GUILD, NUBBY, Some(CHANNEL_A), false);
        let start = startup_last_announced();
        let (announced, last) = decide(None, &new, start);
        assert!(!announced);
        assert_eq!(last, start);
    }

    #[test]
    fn second_join_within_window_does_not_announce_and_keeps_timestamp() {
        let mut last = startup_last_announced();
        let join = in_channel(NUBBY, CHANNEL_A);
        assert!(should_announce(None, &join, watch(), &mut last, now()));
        assert_eq!(last, now());

        // Leaves, then rejoins an hour later.
        let later = now() + Duration::hours(1);
        assert!(!should_announce(None, &join, watch(), &mut last, later));
        assert_eq!(last, now(), "timestamp must not move on a suppressed join");
    }

    #[test]
    fn join_exactly_12h_after_last_announcement_does_not_announce() {
        let start = now() - Duration::hours(NUBBY_ANNOUNCE_RESET_HOURS);
        let (announced, last) = decide(None, &in_channel(NUBBY, CHANNEL_A), start);
        assert!(!announced);
        assert_eq!(last, start);
    }

    #[test]
    fn join_12h_and_1s_after_last_announcement_announces() {
        let start = now() - Duration::hours(NUBBY_ANNOUNCE_RESET_HOURS) - Duration::seconds(1);
        let (announced, last) = decide(None, &in_channel(NUBBY, CHANNEL_A), start);
        assert!(announced);
        assert_eq!(last, now());
    }
}
