use crate::config::AppConfig;
use anyhow::Result;

mod commands;
pub mod parser;
pub mod utils;

#[allow(unused_imports)]
use commands::*;
pub use parser::{AliasSubcommand, Cli, Commands, EnvCommands};

impl Cli {
    pub async fn execute(self, config: AppConfig) -> Result<()> {
        match self.command {
            Commands::Connect {
                target,
                user,
                port,
                kerberos,
                bastion,
                no_bastion,
                bastion_user,
                key,
            } => {
                commands::connect::execute(
                    target,
                    user,
                    port,
                    kerberos,
                    bastion,
                    no_bastion,
                    bastion_user,
                    key,
                    config,
                )
                .await
            }
            Commands::Add {
                name,
                host,
                user,
                port,
                kerberos,
                bastion,
                no_bastion,
                bastion_user,
                key,
                tags,
            } => {
                commands::add::execute(
                    name,
                    host,
                    user,
                    port,
                    kerberos,
                    bastion,
                    no_bastion,
                    bastion_user,
                    key,
                    tags,
                    config,
                )
                .await
            }
            Commands::List {
                tag,
                recent,
                detailed,
            } => commands::list::execute(tag, recent, detailed, config).await,
            Commands::Remove { target, force } => {
                commands::remove::execute(target, force, config).await
            }
            Commands::Show { target } => commands::show::execute(target, config).await,
            Commands::Edit {
                target,
                name,
                host,
                user,
                port,
                kerberos,
                bastion,
                no_bastion,
                bastion_user,
                key,
                add_tags,
                remove_tags,
            } => {
                commands::edit::execute(
                    target,
                    name,
                    host,
                    user,
                    port,
                    kerberos,
                    bastion,
                    no_bastion,
                    bastion_user,
                    key,
                    add_tags,
                    remove_tags,
                    config,
                )
                .await
            }
            Commands::Config {
                default_user,
                default_bastion,
                default_bastion_user,
                default_port,
                use_kerberos,
                log_level,
                clear_bastion,
                search_mode,
            } => {
                commands::config::execute(
                    default_user,
                    default_bastion,
                    default_bastion_user,
                    default_port,
                    use_kerberos,
                    log_level,
                    clear_bastion,
                    search_mode,
                    config,
                )
                .await
            }
            Commands::Stats => commands::stats::execute(config).await,
            Commands::Export {
                format,
                output,
                tag,
            } => commands::export::execute(format, output, tag, config).await,
            Commands::Backup { output } => commands::backup::execute(output, config).await,
            Commands::Doctor => commands::doctor::execute(config).await,
            Commands::Restore { file, force } => {
                commands::restore::execute(file, force, config).await
            }
            Commands::Duplicate { source, new_name } => {
                commands::duplicate::execute(source, new_name, config).await
            }
            Commands::Ping { target, timeout } => {
                commands::ping::execute(target, timeout, config).await
            }
            Commands::Groups { group_name } => commands::groups::execute(group_name, config).await,
            Commands::Env { command } => commands::env::execute(command).await,
            Commands::Import { file, no_bastion } => {
                commands::import::execute(file, no_bastion, config).await
            }
            Commands::Completions { shell } => commands::completions::execute(shell, config).await,
            Commands::History {
                connection,
                limit,
                days,
                failed,
            } => commands::history::execute(connection, limit, days, failed, config).await,
            Commands::Tui => commands::tui::execute(config).await,
            Commands::Exec { target, command } => {
                commands::exec::execute(target, command, config).await
            }
            Commands::Upload {
                target,
                local,
                remote,
                offset,
                mode,
                recursive,
            } => {
                commands::transfer::execute_upload(
                    target, local, remote, offset, mode, recursive, config,
                )
                .await
            }
            Commands::Download {
                target,
                remote,
                local,
                recursive,
            } => {
                commands::transfer::execute_download(target, remote, local, recursive, config).await
            }
            Commands::Forward { target, local } => {
                commands::forward::execute(target, local, config).await
            }
            Commands::Proxy {
                target,
                dynamic,
                bind,
            } => commands::proxy::execute(target, dynamic, bind, config).await,
            Commands::Alias { action } => {
                let alias_action = match action {
                    AliasSubcommand::Add { alias, target } => {
                        commands::alias::AliasAction::Add { alias, target }
                    }
                    AliasSubcommand::Remove { alias } => {
                        commands::alias::AliasAction::Remove { alias }
                    }
                    AliasSubcommand::List { target } => {
                        commands::alias::AliasAction::List { target }
                    }
                };
                commands::alias::execute(alias_action, config).await
            }
            Commands::Close {
                target,
                all,
                cleanup,
                force,
            } => commands::close::execute(target, all, cleanup, force, config).await,
        }
    }
}
