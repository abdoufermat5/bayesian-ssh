use crate::cli::utils::fuzzy_select_connection;
use crate::cli::Cli;
use crate::config::AppConfig;
use crate::database::{Database, RankContext};
use crate::services::SshService;
use anyhow::Result;
use clap::CommandFactory;
use std::io::{self, IsTerminal, Write};

pub async fn execute(config: AppConfig) -> Result<()> {
    let db = Database::new(&config)?;
    let ctx = if config.prediction.enabled {
        RankContext::current(&db, &config)
    } else {
        RankContext::default()
    };

    let predictions = db.predict(&ctx, 3)?;
    if predictions.is_empty() {
        Cli::command().print_help()?;
        println!();
        return Ok(());
    }

    let prev_name = ctx
        .prev_connection_id
        .and_then(|id| db.get_connection(&id.to_string()).ok().flatten())
        .map(|c| c.name);

    let name_width = predictions
        .iter()
        .map(|p| p.connection.name.chars().count())
        .max()
        .unwrap_or(8)
        .max(8);
    let addr_width = predictions
        .iter()
        .map(|p| {
            format!("{}@{}", p.connection.user, p.connection.host)
                .chars()
                .count()
        })
        .max()
        .unwrap_or(12)
        .max(12);

    println!("Suggested connections");
    for (i, pred) in predictions.iter().enumerate() {
        let addr = format!("{}@{}", pred.connection.user, pred.connection.host);
        let reason = pred.format_reason(&ctx, prev_name.as_deref());
        println!(
            " {}. {:<name_w$}  {:<addr_w$}  · {}",
            i + 1,
            pred.connection.name,
            addr,
            reason,
            name_w = name_width,
            addr_w = addr_width,
        );
    }

    if !io::stdin().is_terminal() {
        return Ok(());
    }

    let ssh_service = SshService::new(config.clone())?;

    loop {
        print!("Enter = 1 · number · text to search · q to quit: ");
        io::stdout().flush()?;

        let mut line = String::new();
        if io::stdin().read_line(&mut line)? == 0 {
            return Ok(());
        }
        let trimmed = line.trim();

        if trimmed.is_empty() {
            return ssh_service
                .connect_to_connection(
                    &predictions[0].connection,
                    None,
                    None,
                    None,
                    None,
                    false,
                    None,
                    None,
                    None,
                )
                .await;
        }

        if trimmed.eq_ignore_ascii_case("q") || trimmed.eq_ignore_ascii_case("quit") {
            return Ok(());
        }

        if let Ok(index) = trimmed.parse::<usize>() {
            if index >= 1 && index <= predictions.len() {
                return ssh_service
                    .connect_to_connection(
                        &predictions[index - 1].connection,
                        None,
                        None,
                        None,
                        None,
                        false,
                        None,
                        None,
                        None,
                    )
                    .await;
            }
            println!(
                "❌ Invalid selection. Enter 1-{}, search text, or 'q' to quit.",
                predictions.len()
            );
            continue;
        }

        if let Some(selected) =
            fuzzy_select_connection(&ssh_service, trimmed, "connect to", true, &config).await?
        {
            return ssh_service
                .connect_to_connection(&selected, None, None, None, None, false, None, None, None)
                .await;
        }
        return Ok(());
    }
}
