use crate::cli::Cli;
use crate::cli::utils::fuzzy_select_connection;
use crate::config::AppConfig;
use crate::database::{Database, RankContext};
use crate::services::{ConnectOverrides, SshService};
use crate::ui::{Glyph, Style, Table, Tone, heading};
use anyhow::Result;
use clap::CommandFactory;
use std::io::{self, IsTerminal, Write};

pub async fn execute(config: AppConfig) -> Result<()> {
    let style = Style::terminal();
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
        .and_then(|id| db.connection(&id.to_string()).ok().flatten())
        .map(|c| c.name);

    println!("{}", heading(&style, "Suggested Connections", None));

    let mut table = Table::new(&["#", "NAME", "TARGET", "REASON"]).align_right(0);
    for (i, pred) in predictions.iter().enumerate() {
        let addr = format!("{}@{}", pred.connection.user, pred.connection.host);
        let reason = pred.format_reason(&ctx, prev_name.as_deref());
        table.row(vec![
            style.paint(Tone::Info, &(i + 1).to_string()),
            style.paint(Tone::Strong, &pred.connection.name),
            style.paint(Tone::Dim, &addr),
            style.paint(Tone::Dim, &reason),
        ]);
    }
    println!("{}", table.render(&style).trim_end_matches('\n'));

    if !io::stdin().is_terminal() {
        return Ok(());
    }

    let ssh_service = SshService::new(config.clone())?;
    let dot = style.glyph(Glyph::Dot);
    let prompt = style.paint(
        Tone::Dim,
        &format!("Enter = 1 {dot} number {dot} text to search {dot} q to quit: "),
    );

    loop {
        print!("{prompt}");
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
                    ConnectOverrides {
                        user: None,
                        port: None,
                        kerberos: None,
                        bastion: None,
                        no_bastion: false,
                        bastion_user: None,
                        key: None,
                        record: None,
                    },
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
                        ConnectOverrides {
                            user: None,
                            port: None,
                            kerberos: None,
                            bastion: None,
                            no_bastion: false,
                            bastion_user: None,
                            key: None,
                            record: None,
                        },
                    )
                    .await;
            }
            println!(
                "{} Invalid selection. Enter 1-{}, search text, or 'q' to quit.",
                style.mark(Glyph::Bad),
                predictions.len()
            );
            continue;
        }

        if let Some(selected) =
            fuzzy_select_connection(&ssh_service, trimmed, "connect to", true, &config).await?
        {
            return ssh_service
                .connect_to_connection(
                    &selected,
                    ConnectOverrides {
                        user: None,
                        port: None,
                        kerberos: None,
                        bastion: None,
                        no_bastion: false,
                        bastion_user: None,
                        key: None,
                        record: None,
                    },
                )
                .await;
        }
        return Ok(());
    }
}
