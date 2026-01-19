use std::sync::Arc;
use deadpool_postgres::GenericClient;
use markovchain_client_lib::content_string::ContentString;
use markovchain_client_lib::{GeneratePayload, InputPayload};
use rand::prelude::Distribution;
use serenity::all::{Context, Message, Ready};
use serenity::async_trait;
use serenity::builder::CreateMessage;
use serenity::prelude::EventHandler;
use tracing::{debug, error, info, warn};
use crate::AppState;

pub struct Handler {
    pub(crate) app_state: Arc<AppState>,
}

#[async_trait]
impl EventHandler for Handler {
    async fn message(&self, ctx: Context, new_message: Message) {
        debug!("Message from {}", new_message.author.name);

        // Save the input only from normal users
        if !new_message.author.bot {
            save_input(&self.app_state, new_message.content.as_str()).await;
        }

        // Prevent possible mention loop, answer only to other users
        if new_message.author.id != ctx.cache.current_user().id {
            answer_to_mention(&self.app_state, &ctx, &new_message).await;
        }

        // Try to generate the text if auto generation is enabled in the channel
        generate_from_interval(&self.app_state, &ctx, &new_message).await;
    }

    async fn ready(&self, _ctx: Context, data_about_bot: Ready) {
        info!(
            "I am @{}. I am on {} guilds.",
            data_about_bot.user.name,
            data_about_bot.guilds.len()
        );
    }
}

async fn save_input(app_state: &AppState, input: &str) {
    let Ok(input) = ContentString::try_from(input) else {
        return;
    };

    let payload = InputPayload { input };

    if let Err(err) = app_state.markov_chain.input(payload).await {
        warn!("Failed to execute `input`: {err}");
    }
}

async fn answer_to_mention(app_state: &AppState, ctx: &Context, message: &Message) {
    if !message.mentions_me(ctx).await.unwrap_or(false) {
        return;
    }

    let _ = message.channel_id.broadcast_typing(ctx).await;

    let text = match continue_message(app_state, message).await {
        Ok(text) => text,
        Err(err) => {
            warn!("Failed to generate a text for mention: {err}");
            return;
        }
    };

    if let Err(err) = message.reply_ping(ctx, text).await {
        error!("Failed to reply: {err}");
    }
}

async fn generate_from_interval(app_state: &AppState, ctx: &Context, message: &Message) {
    let Some(guild_id) = message.guild_id else {
        return;
    };

    let guild_id = i64::try_from(guild_id.get()).unwrap();
    let channel_id = i64::try_from(message.channel_id.get()).unwrap();

    let should_generate = update_gen_interval(app_state, guild_id, channel_id).await.expect("Failed to update generation interval");
    if !should_generate { return; }

    let _ = message.channel_id.broadcast_typing(ctx).await;

    let text = continue_message(app_state, message).await.expect("Failed to generate the text");
    message.channel_id.send_message(ctx, CreateMessage::new().content(text)).await.expect("Failed to send auto-generated text");
}

async fn update_gen_interval(app_state: &AppState, guild_id: i64, channel_id: i64) -> anyhow::Result<bool> {
    let client = app_state.db_pool.get().await.expect("Failed to get the db client from the pool");

    let statement = client.prepare_cached(/* language=postgresql */ r"
        UPDATE enabled_guilds
        SET msgs_until_gen = msgs_until_gen - 1
        WHERE guild_id = $1 AND channel_id = $2
        RETURNING msgs_until_gen, interval;
    ").await?;

    let Some(row) = client
        .query_opt(&statement, &[&guild_id, &channel_id])
        .await? else { return Ok(false); };

    let count: i16 = row.get("msgs_until_gen");

    // reset the `msgs_until_gen` if it is <= 0
    if count <= 0 {
        let interval: Option<i16> = row.get("interval");
        let new_count = interval.map_or_else(|| app_state.uniform.sample(&mut rand::rng()) as i16, |iv| iv);

        let statement = client.prepare_cached(/* language=postgresql */ r"
            UPDATE enabled_guilds
            SET msgs_until_gen = $3
            WHERE guild_id = $1 AND channel_id = $2;
        ").await?;

        client
            .execute(&statement, &[&guild_id, &channel_id, &new_count])
            .await?;
    }

    Ok(count == 0)
}

async fn continue_message(app_state: &AppState, message: &Message) -> anyhow::Result<String> {
    let start = message
        .content
        .split(' ')
        .next_back()
        .and_then(|s| ContentString::try_from(s).ok());

    let mut text = generate(app_state, start, None)
        .await
        .map(|text| text.split_once(' ').unwrap_or((text.as_str(), "")).1.into());

    if text.as_ref().is_ok_and(|text: &String| text.is_empty()) {
        text = generate(app_state, None, None).await;
    }

    text
}

async fn generate(app_state: &AppState, start: Option<ContentString>, max_length: Option<usize>) -> anyhow::Result<String> {
    let payload = GeneratePayload { start, max_length };
    let text = app_state.markov_chain.generate(payload).await?;
    Ok(text)
}
