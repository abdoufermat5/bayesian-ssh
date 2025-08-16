use anyhow::Result;
use crate::config::AppConfig;
use crate::services::SshService;
use tracing::info;

pub async fn execute(config: AppConfig) -> Result<()> {
    info!("Showing application statistics");
    
    let ssh_service = SshService::new(config)?;
    let stats = ssh_service.get_stats().await?;
    
    println!("📊 Bayesian SSH Statistics\n");
    
    println!("Total Connections: {}", stats.total_connections);
    
    if let Some(most_used) = stats.most_used {
        println!("Most Used: {} (last: {})", 
            most_used.name,
            most_used.last_used
                .map(|d| d.format("%Y-%m-%d %H:%M:%S").to_string())
                .unwrap_or_else(|| "Never".to_string())
        );
    }
    
    if !stats.recently_used.is_empty() {
        println!("\nRecently Used:");
        for (i, conn) in stats.recently_used.iter().take(5).enumerate() {
            if let Some(last_used) = conn.last_used {
                println!("  {}. {} - {}", 
                    i + 1, 
                    conn.name, 
                    last_used.format("%m-%d %H:%M")
                );
            }
        }
    }
    
    if !stats.by_tag.is_empty() {
        println!("\nConnections by Tag:");
        let mut tag_vec: Vec<_> = stats.by_tag.iter().collect();
        tag_vec.sort_by(|a, b| b.1.cmp(a.1));
        
        for (tag, count) in tag_vec.iter().take(10) {
            println!("  {}: {} connection(s)", tag, count);
        }
    }
    
    Ok(())
}
