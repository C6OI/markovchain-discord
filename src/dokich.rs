use chrono::{DateTime, Duration, FixedOffset, Utc};
use serenity::all::{Context, Member, Message, Timestamp};
use serenity::async_trait;
use serenity::prelude::EventHandler;
use tokio::sync::Mutex;
use tracing::{error, warn};

const MESSAGES_PER_DAY: usize = 25;
const GUILD_ID: u64 = 1196050317607972934;
const USER_ID: u64 = 469074287236743171;

pub struct DokichHandler {
    messages_count: Mutex<usize>,
    reset_at_unix: Mutex<i64>,
}

impl Default for DokichHandler {
    fn default() -> Self {
        Self {
            messages_count: Mutex::new(0),
            reset_at_unix: Mutex::new(next_midnight_utc_plus_3_unix()),
        }
    }
}

#[async_trait]
impl EventHandler for DokichHandler {
    async fn message(&self, ctx: Context, new_message: Message) {
        if new_message.guild_id.is_none_or(|id| id != GUILD_ID) || new_message.author.id != USER_ID
        {
            return;
        }

        if let Ok(mut member) = new_message.member(&ctx.http).await {
            let mut messages_count = self.messages_count.lock().await;
            let mut reset_at_unix = self.reset_at_unix.lock().await;
            let now_unix = Utc::now().timestamp();

            if now_unix >= *reset_at_unix {
                *messages_count = 0;
                *reset_at_unix = next_midnight_utc_plus_3_unix();
            }

            *messages_count += 1;

            warn!("Messages count: {messages_count}");

            if *messages_count >= MESSAGES_PER_DAY {
                let disable_until = Timestamp::from_unix_timestamp(*reset_at_unix)
                    .expect("next reset timestamp must be valid");

                if let Err(err) =
                    disable_communication(&mut member, &ctx, &new_message, disable_until).await
                {
                    error!("Error disabling communication: {err:#}");
                }

                warn!("Disabled communication until {disable_until}");
            }
        }
    }
}

async fn disable_communication(
    member: &mut Member,
    ctx: &Context,
    new_message: &Message,
    time: Timestamp,
) -> anyhow::Result<()> {
    use anyhow::Context;

    member
        .disable_communication_until_datetime(&ctx.http, time)
        .await
        .context("Failed to disable communication")?;

    new_message
        .reply_ping(
            &ctx.http,
            "https://tenor.com/view/stfu-gif-6401003389838608981",
        )
        .await
        .context("Failed to send reply message")?;

    Ok(())
}

fn next_midnight_utc_plus_3_unix() -> i64 {
    let tz = FixedOffset::east_opt(3 * 3600).expect("UTC+3 offset must be valid");
    let now_local = Utc::now().with_timezone(&tz);
    let next_day = now_local.date_naive() + Duration::days(1);
    let next_midnight_local = next_day
        .and_hms_opt(0, 0, 0)
        .expect("midnight time must be valid");
    let next_midnight_utc: DateTime<Utc> = next_midnight_local.and_utc() - Duration::hours(3);
    next_midnight_utc.timestamp()
}
