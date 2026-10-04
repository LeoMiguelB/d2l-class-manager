mod auth;
mod cli;
mod client;
mod config;
mod models;
mod reconcile;
mod vault;

use auth::StoredToken;
use clap::Parser;
use cli::{Cli, Commands};
use client::D2LClient;
use config::AppConfig;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Cli::parse();
    let config = AppConfig::new(args.host, args.vault)?;

    // Handle Login separately (doesn't require an existing token)
    if let Commands::Login { headless, force, token } = args.command {
        if let Some(tok_str) = token {
            let stored = StoredToken::from_jwt(&config.host, &tok_str)?;
            stored.save_to_file(&config.token_path)?;
            println!("✅ Token saved successfully to {:?}", config.token_path);
            return Ok(());
        }

        println!("Authenticating with D2L Brightspace ({host})...", host = config.host);
        let token = auth::resolve_token(&config, force, headless).await?;
        println!("✅ Authentication successful! Logged in as: {}", token.sub.as_deref().unwrap_or("authenticated user"));
        return Ok(());
    }

    // All other commands require a valid token
    let token = match auth::resolve_token(&config, false, true).await {
        Ok(t) => t,
        Err(e) => {
            eprintln!("Authentication error: {}", e);
            eprintln!("Please run `d2l login` to authenticate.");
            std::process::exit(1);
        }
    };

    let client = D2LClient::new(&config.host, token)?;

    match args.command {
        Commands::Login { .. } => unreachable!(),
        Commands::Status => {
            cli::handlers::handle_status(&client, args.json).await?;
        }
        Commands::Courses { active } => {
            cli::handlers::handle_courses(&client, active, args.json).await?;
        }
        Commands::Announcements { course, since } => {
            cli::handlers::handle_announcements(&client, &config, course, since, args.json).await?;
        }
        Commands::Assignments { course } => {
            cli::handlers::handle_assignments(&client, &config, course, args.json).await?;
        }
        Commands::Download { course, kind, dest } => {
            cli::handlers::handle_download(&client, &config, course, kind, dest, args.json).await?;
        }
        Commands::Posts { course, forum, topic } => {
            cli::handlers::handle_posts(&client, &config, course, forum, topic, args.json).await?;
        }
        Commands::Calendar { days } => {
            cli::handlers::handle_calendar(&client, &config, days, args.json).await?;
        }
        Commands::Quizzes { course } => {
            cli::handlers::handle_quizzes(&client, &config, course, args.json).await?;
        }
        Commands::Grades { course } => {
            cli::handlers::handle_grades(&client, &config, course, args.json).await?;
        }
        Commands::Dump { course } => {
            cli::handlers::handle_dump(&client, &config, course).await?;
        }
        Commands::Reconcile { course, apply } => {
            cli::handlers::handle_reconcile(&client, &config, course, apply, args.json).await?;
        }
    }

    Ok(())
}
