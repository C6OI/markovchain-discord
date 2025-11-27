#![warn(clippy::all, clippy::nursery, clippy::pedantic)]

mod event_handler;
mod settings;
mod commands;
mod migrations;
mod database;

use crate::event_handler::Handler;
use crate::settings::Settings;
use markovchain_client_lib::MarkovChainClient;
use serenity::all::GatewayIntents;
use std::env;
use std::io::stdout;
use std::path::Path;
use std::sync::Arc;
use deadpool_postgres::Pool;
use rand::distr::Uniform;
use tracing::info;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;
use crate::commands::collect_commands;
use crate::database::create_pool;
use crate::migrations::Migrations;

#[allow(unused)]
pub struct AppState {
    settings: Settings,
    db_pool: Pool,
    markov_chain: MarkovChainClient,
    uniform: Uniform<u8>
}

#[tokio::main]
async fn main() {
    #[rustfmt::skip]
    let env_filter = EnvFilter::builder().parse_lossy(
        env::var("RUST_LOG")
          .as_deref()
          .unwrap_or("info"),
    );

    let file_appender = tracing_appender::rolling::hourly("logs", "rolling.log");
    let (non_blocking_file, _file_guard) = tracing_appender::non_blocking(file_appender);
    let (non_blocking_stdout, _stdout_guard) = tracing_appender::non_blocking(stdout());
    let console = tracing_subscriber::fmt::layer().with_writer(non_blocking_stdout);

    let file = tracing_subscriber::fmt::layer()
        .json()
        .with_ansi(false)
        .with_writer(non_blocking_file);

    tracing_subscriber::registry()
        .with(env_filter)
        .with(console)
        .with(file)
        .init();

    info!("Welcome to {}", env!("CARGO_PKG_NAME"));

    let settings = Settings::parse().expect("Failed to load settings");

    let db_pool = create_pool(&settings.database).await.expect("Failed to create pool");
    let client = db_pool.get().await.expect("Failed to get client from pool");

    Migrations::new("version_info".into(), Path::new("migrations"))
        .expect("Failed to initialize migrations")
        .up(&client)
        .await
        .expect("Failed to apply migrations");

    let intents = GatewayIntents::GUILD_MESSAGES
        | GatewayIntents::MESSAGE_CONTENT
        | GatewayIntents::DIRECT_MESSAGES;

    let app_state = Arc::new(AppState {
        markov_chain: MarkovChainClient::new(settings.server.url.clone()),
        uniform: Uniform::try_from(5..=50).unwrap(),
        settings,
        db_pool,
    });

    let app_state_clone = app_state.clone();

    let framework = poise::Framework::builder()
        .options(poise::FrameworkOptions {
            commands: collect_commands(),
            ..Default::default()
        })
        .setup(|ctx, _ready, framework| {
            Box::pin(async move {
                poise::builtins::register_globally(ctx, &framework.options().commands).await?;
                Ok(app_state_clone)
            })
        })
        .build();

    let mut discord = serenity::Client::builder(&app_state.settings.discord.token, intents)
        .event_handler(Handler { app_state })
        .framework(framework)
        .await
        .expect("Failed to build Discord client");

    discord.start().await.expect("Discord client error");
}
