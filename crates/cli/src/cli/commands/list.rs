use crate::cli::utils::format_duration;
use crate::config::AppConfig;
use crate::services::SshService;
use crate::ui::{Glyph, KvList, Style, Table, Tone, heading};
use anyhow::Result;
use tracing::info;

pub async fn execute(
    tag: Option<String>,
    recent: bool,
    detailed: bool,
    config: AppConfig,
) -> Result<()> {
    info!(
        "Listing connections (tag: {:?}, recent: {}, detailed: {})",
        tag, recent, detailed
    );

    let style = Style::terminal();
    let ssh_service = SshService::new(config.clone())?;
    let connections = ssh_service.list_connections(tag.as_deref(), recent).await?;
    let source_names: std::collections::HashMap<_, _> = crate::database::Database::new(&config)?
        .list_sources()?
        .into_iter()
        .map(|summary| (summary.source.id, summary.source.name))
        .collect();

    if connections.is_empty() {
        println!("{} No connections found.", style.mark(Glyph::Dot));
        if tag.is_some() {
            println!(
                "  {}",
                style.paint(
                    Tone::Dim,
                    "Try without --tag filter or use 'bssh add' to create connections."
                )
            );
        }
        return Ok(());
    }

    let dot = style.glyph(Glyph::Dot);
    let meta = match (&tag, recent) {
        (Some(t), true) => format!("{} {dot} tag: {t}, recent", connections.len()),
        (Some(t), false) => format!("{} {dot} tag: {t}", connections.len()),
        (None, true) => format!("{} {dot} recent", connections.len()),
        (None, false) => format!("{}", connections.len()),
    };

    println!("{}", heading(&style, "Connections", Some(&meta)));

    if detailed {
        for conn in &connections {
            let mut badges = Vec::new();
            if conn.use_kerberos {
                badges.push(style.paint(Tone::Info, "krb5"));
            }
            if conn.bastion.is_some() {
                badges.push(style.paint(Tone::Info, "jump"));
            }
            if conn.stale_since.is_some() {
                badges.push(style.paint(Tone::Warn, "stale"));
            }

            if badges.is_empty() {
                println!("\n{}", style.paint(Tone::Strong, &conn.name));
            } else {
                println!(
                    "\n{} {}",
                    style.paint(Tone::Strong, &conn.name),
                    badges.join(" ")
                );
            }

            let mut kv = KvList::new();
            kv.push("address", format!("{}@{}:{}", conn.user, conn.host, conn.port));

            if let Some(bastion) = &conn.bastion {
                kv.push(
                    "jump",
                    format!(
                        "{}@{}",
                        conn.bastion_user.as_deref().unwrap_or(&conn.user),
                        bastion
                    ),
                );
            }

            if let Some(key) = &conn.key_path {
                kv.push("key", key.clone());
            }

            if !conn.tags.is_empty() {
                kv.push("tags", style.paint(Tone::Dim, &conn.tags.join(", ")));
            }

            if let Some(source) = conn.source_id.and_then(|id| source_names.get(&id)) {
                kv.push("source", style.paint(Tone::Dim, source));
            }

            if let Some(stale_since) = conn.stale_since {
                kv.push(
                    "stale since",
                    style.paint(Tone::Warn, &format_duration(stale_since)),
                );
            }

            if let Some(last_used) = conn.last_used {
                kv.push(
                    "last used",
                    style.paint(Tone::Dim, &format_duration(last_used)),
                );
            }

            kv.push("id", style.paint(Tone::Dim, &conn.id.to_string()));
            println!("{}", kv.render(&style).trim_end_matches('\n'));
        }
    } else {
        let mut table = Table::new(&["#", "NAME", "TARGET", "PORT", "INFO"])
            .align_right(0)
            .align_right(3);

        for (i, conn) in connections.iter().enumerate() {
            let name_cell = if conn.stale_since.is_some() {
                format!(
                    "{} {}",
                    style.paint(Tone::Strong, &conn.name),
                    style.paint(Tone::Warn, "(stale)")
                )
            } else {
                style.paint(Tone::Strong, &conn.name)
            };

            let target_cell = format!("{}@{}", conn.user, conn.host);
            let port_cell = style.paint(Tone::Dim, &conn.port.to_string());

            let mut info_parts = Vec::new();
            if conn.use_kerberos {
                info_parts.push(style.paint(Tone::Info, "krb5"));
            }
            if conn.bastion.is_some() {
                info_parts.push(style.paint(Tone::Info, "jump"));
            }
            if let Some(source) = conn.source_id.and_then(|id| source_names.get(&id)) {
                info_parts.push(style.paint(Tone::Dim, &format!("[{source}]")));
            }
            if let Some(last_used) = conn.last_used {
                info_parts.push(style.paint(Tone::Dim, &format_duration(last_used)));
            }
            if !conn.tags.is_empty() {
                info_parts.push(style.paint(Tone::Dim, &format!("[{}]", conn.tags.join(","))));
            }

            table.row(vec![
                style.paint(Tone::Dim, &(i + 1).to_string()),
                name_cell,
                target_cell,
                port_cell,
                info_parts.join(" "),
            ]);
        }

        println!("{}", table.render(&style).trim_end_matches('\n'));
    }

    println!(
        "\n{}",
        style.paint(
            Tone::Dim,
            &format!("Total: {} connection(s)", connections.len())
        )
    );

    Ok(())
}
