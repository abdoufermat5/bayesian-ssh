use crate::config::AppConfig;
use crate::services::SshService;
use crate::ui::{KvList, Style, Table, Tone, heading};
use anyhow::Result;
use tracing::info;

pub async fn execute(config: AppConfig) -> Result<()> {
    info!("Showing application statistics");

    let style = Style::terminal();
    let ssh_service = SshService::new(config)?;
    let stats = ssh_service.stats().await?;

    println!("{}", heading(&style, "Bayesian SSH Statistics", None));

    let mut summary = KvList::new();
    summary.push(
        "Total connections",
        style.paint(Tone::Strong, &stats.total_connections.to_string()),
    );

    if let Some(most_used) = stats.most_used {
        let last = most_used
            .last_used
            .map(|d| d.format("%Y-%m-%d %H:%M:%S").to_string())
            .unwrap_or_else(|| "Never".to_string());
        summary.push(
            "Most used",
            format!(
                "{} {}",
                style.paint(Tone::Strong, &most_used.name),
                style.paint(Tone::Dim, &format!("(last: {last})"))
            ),
        );
    }

    println!("{}", summary.render(&style).trim_end_matches('\n'));

    if !stats.recently_used.is_empty() {
        let mut table = Table::new(&["#", "CONNECTION", "LAST USED"]).align_right(0);
        for (i, conn) in stats.recently_used.iter().take(5).enumerate() {
            if let Some(last_used) = conn.last_used {
                table.row(vec![
                    style.paint(Tone::Dim, &(i + 1).to_string()),
                    style.paint(Tone::Strong, &conn.name),
                    style.paint(Tone::Dim, &last_used.format("%Y-%m-%d %H:%M").to_string()),
                ]);
            }
        }
        if !table.is_empty() {
            println!("\n{}", heading(&style, "Recently Used", None));
            println!("{}", table.render(&style).trim_end_matches('\n'));
        }
    }

    if !stats.by_tag.is_empty() {
        let mut tag_vec: Vec<_> = stats.by_tag.iter().collect();
        // Most-used first; ties by name so the output (and which tags make
        // the top ten) doesn't depend on HashMap iteration order.
        tag_vec.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));

        let mut table = Table::new(&["TAG", "CONNECTIONS"]).align_right(1);
        for (tag, count) in tag_vec.iter().take(10) {
            table.row(vec![style.paint(Tone::Info, tag), count.to_string()]);
        }

        println!("\n{}", heading(&style, "Connections by Tag", None));
        println!("{}", table.render(&style).trim_end_matches('\n'));
    }

    Ok(())
}
