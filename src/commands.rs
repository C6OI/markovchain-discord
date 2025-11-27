use rand::distr::Distribution;
use std::sync::Arc;
use markovchain_client_lib::content_string::ContentString;
use markovchain_client_lib::GeneratePayload;
use poise::Command;
use serenity::all::Message;
use crate::AppState;

pub type Error = Box<dyn std::error::Error + Send + Sync>;
pub type Context<'a> = poise::Context<'a, Arc<AppState>, Error>;

pub fn collect_commands() -> Vec<Command<Arc<AppState>, Error>> {
    vec![
        generate(),
        enable_gen(),
        disable_gen(),
        interval(),
        continue_context_menu()
    ]
}

/// Generate a nonsense
#[poise::command(slash_command, required_bot_permissions = "SEND_MESSAGES")]
async fn generate(
    ctx: Context<'_>,
    #[description = "Optional, the text to start generating from"] #[min_length = 1] #[max_length = 2000] start: Option<String>,
    #[description = "Optional, the maximum length of the generated text"] #[min = 1] #[max = 2000] max_length: Option<usize>
) -> Result<(), Error> {
    ctx.defer().await?;
    let app_state = ctx.data();

    let payload = GeneratePayload {
        start: start.map(|str| str.try_into().unwrap()),
        max_length
    };

    let text = app_state.markov_chain.generate(payload).await?;

    ctx.reply(text).await?;
    Ok(())
}

/// Enable the automatic generation in this channel
#[poise::command(slash_command, guild_only, required_permissions = "MANAGE_CHANNELS")]
async fn enable_gen(
    ctx: Context<'_>
) -> Result<(), Error> {
    ctx.defer().await?;

    let channel = ctx.guild_channel().await.unwrap();

    let app_state = ctx.data();
    let db = app_state.db_pool.get().await?;

    let statement = db.prepare_cached(/* language=postgresql */ r"
        SELECT EXISTS (
            SELECT 1
            FROM enabled_guilds
            WHERE guild_id = $1 AND channel_id = $2
        );
    ").await?;

    let guild_id = i64::try_from(channel.guild_id.get())?;
    let channel_id = i64::try_from(channel.id.get())?;

    let exists = db
        .query_one(&statement, &[&guild_id, &channel_id])
        .await
        .expect("Failed to execute the exists query")
        .get(0);

    if exists {
        ctx.reply("Automatic generation is already enabled in this channel").await?;
        return Ok(());
    }

    let statement = db.prepare_cached(/* language=postgresql */ r"
        INSERT INTO enabled_guilds (guild_id, channel_id, msgs_until_gen)
        VALUES ($1, $2, $3);
    ").await?;

    let messages_until_generation = i16::from(app_state.uniform.sample(&mut rand::rng()));
    db.execute(&statement, &[&guild_id, &channel_id, &messages_until_generation])
        .await
        .expect("Failed to execute the insert query");

    ctx.reply("Enabled the automatic generation in this channel").await?;
    Ok(())
}

/// Disable the automatic generation in this channel
#[poise::command(slash_command, guild_only, required_permissions = "MANAGE_CHANNELS")]
async fn disable_gen(
    ctx: Context<'_>
) -> Result<(), Error> {
    ctx.defer().await?;

    let channel = ctx.guild_channel().await.unwrap();

    let app_state = ctx.data();
    let db = app_state.db_pool.get().await?;

    let guild_id = i64::try_from(channel.guild_id.get())?;
    let channel_id = i64::try_from(channel.id.get())?;

    let statement = db.prepare_cached(/* language=postgresql */ r"
        DELETE FROM enabled_guilds
        WHERE guild_id = $1 AND channel_id = $2;
    ").await?;

    let disabled = db.execute(&statement, &[&guild_id, &channel_id]).await? > 0;

    if disabled {
        ctx.reply("Disabled the automatic generation in this channel").await?;
    } else {
        ctx.reply("Automatic generation is already disabled in this channel").await?;
    }

    Ok(())
}

/// Set the interval between automatic generations in this channel
#[poise::command(slash_command, guild_only, required_permissions = "MANAGE_CHANNELS")]
async fn interval(
    ctx: Context<'_>,
    #[description = "Interval between generations. Random, if not specified"] #[min = 5] #[max = 50] interval: Option<i16>
) -> Result<(), Error> {
    ctx.defer().await?;

    let channel = ctx.guild_channel().await.unwrap();

    let app_state = ctx.data();
    let db = app_state.db_pool.get().await?;

    let guild_id = i64::try_from(channel.guild_id.get())?;
    let channel_id = i64::try_from(channel.id.get())?;

    let statement = db.prepare_cached(/* language=postgresql */ r"
        UPDATE enabled_guilds
        SET interval = $3,
            msgs_until_gen = least($3, msgs_until_gen)
        WHERE guild_id = $1 AND channel_id = $2;
    ").await?;

    let updated = db.execute(&statement, &[&guild_id, &channel_id, &interval]).await? > 0;

    if updated {
        let interval_text = interval.map_or_else(|| "random".into(), |val| val.to_string());

        ctx.reply(format!("The interval between automatic generations is set to {interval_text} in this channel")).await?;
    } else {
        ctx.reply("Cannot change interval because automatic generation is disabled in this channel").await?;
    }

    Ok(())
}

/// Generate a nonsense from selected message
#[poise::command(context_menu_command = "Continue", required_bot_permissions = "SEND_MESSAGES")]
async fn continue_context_menu(ctx: Context<'_>, message: Message) -> Result<(), Error> {
    ctx.defer().await?;
    let app_state = ctx.data();

    let payload = GeneratePayload {
        max_length: Some((message.content.len() as f64 * 1.5) as usize),
        start: Some(ContentString::new(message.content)?),
    };

    let text = app_state.markov_chain.generate(payload).await?;

    ctx.reply(text).await?;
    Ok(())
}
