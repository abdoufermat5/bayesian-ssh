//! `bssh ai prune`.
//!
//! Removes old runs from the audit log, the recordings, the change backups and the runs
//! database, optionally archiving them first. The work is in `services::ai::prune`; this file
//! turns arguments into a request, shows the plan and asks.

use crate::cli::parser_ai::AiArgsPrune;
use crate::cli::utils::format_bytes;
use crate::config::AppConfig;
use crate::services::ai::prune::{
    self, AiStorage, PruneMode, PruneOutcome, PrunePlan, PruneRequest, PruneSelection, PruneWhat,
};
use crate::services::ai::ui::{self, Glyph, Style, Tone};
use anyhow::{Result, bail};
use std::io::{IsTerminal, Write};

/// Plan, confirm and run a prune, or print the storage summary.
///
/// # Errors
/// Returns an error for a bad combination of options, when confirmation is needed and there
/// is no terminal, when the prune fails, or when the audit chain does not verify afterwards.
pub async fn execute(args: AiArgsPrune, config: AppConfig) -> Result<()> {
    if args.stats {
        let s = prune::storage(&config)?;
        if args.json {
            println!("{}", serde_json::to_string_pretty(&s)?);
        } else {
            print!("{}", render_storage(&Style::terminal(), &s));
        }
        return Ok(());
    }
    let req = request(&args)?;
    let plan = prune::plan(&config, &req)?;
    let style = Style::terminal();
    if args.dry_run {
        if args.json {
            println!("{}", serde_json::to_string_pretty(&plan)?);
        } else {
            print!("{}", render_plan(&style, &plan));
            println!("Dry run: nothing was changed.");
        }
        return Ok(());
    }
    if plan.runs.is_empty() && req.mode != PruneMode::Wipe {
        if args.json {
            println!("{}", serde_json::to_string_pretty(&plan)?);
        } else {
            print!("{}", render_plan(&style, &plan));
        }
        return Ok(());
    }
    if !args.yes {
        if args.json || !std::io::stdin().is_terminal() {
            bail!(
                "refusing to prune without --yes when there is no terminal to ask (see --dry-run)"
            );
        }
        print!("{}", render_plan(&style, &plan));
        if !confirm(&question(&plan))? {
            println!("Nothing was changed.");
            return Ok(());
        }
    }
    let outcome = tokio::task::spawn_blocking(move || prune::execute(&config, &req)).await??;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&outcome)?);
    } else {
        print!("{}", render_outcome(&style, &outcome));
    }
    if !outcome.verify_ok {
        bail!("the audit log does not verify after the prune: keep the archive and report this");
    }
    Ok(())
}

/// What the arguments ask for.
fn request(args: &AiArgsPrune) -> Result<PruneRequest> {
    let chosen = usize::from(args.older_than.is_some())
        + usize::from(!args.run.is_empty())
        + usize::from(args.all);
    if chosen != 1 {
        bail!("choose exactly one of --older-than <AGE>, --run <ID> or --all");
    }
    let selection = if let Some(age) = &args.older_than {
        PruneSelection::OlderThan {
            days: parse_age(age)?,
        }
    } else if !args.run.is_empty() {
        PruneSelection::Runs {
            ids: args.run.clone(),
        }
    } else {
        PruneSelection::All
    };
    let narrowed = args.keep_recordings || args.keep_backups || args.keep_runs_db;
    let (mode, archive_dir) = match (&args.archive, args.wipe) {
        (_, true) if !args.all => bail!("--wipe removes everything: use it with --all"),
        (Some(_), true) => bail!("--wipe and --archive don't combine: archive, then wipe"),
        (_, true) if narrowed => bail!("--wipe removes everything: drop the --keep-* options"),
        (_, true) => (PruneMode::Wipe, None),
        (Some(dir), false) => (PruneMode::Archive, dir.clone()),
        (None, false) => (PruneMode::Delete, None),
    };
    Ok(PruneRequest {
        selection,
        mode,
        what: PruneWhat {
            audit: true,
            recordings: !args.keep_recordings,
            backups: !args.keep_backups,
            runs_db: !args.keep_runs_db,
        },
        archive_dir,
    })
}

/// `30d`, `12h`, `2w` or a bare number of days, as whole days rounded up.
fn parse_age(s: &str) -> Result<u32> {
    let s = s.trim();
    let (digits, hours_per_unit) = match s.chars().last() {
        Some('h') => (&s[..s.len() - 1], 1),
        Some('d') => (&s[..s.len() - 1], 24),
        Some('w') => (&s[..s.len() - 1], 24 * 7),
        _ => (s, 24),
    };
    let n: u64 = digits
        .parse()
        .map_err(|_| anyhow::anyhow!("'{s}' is not an age: use 30d, 12h or 2w"))?;
    let days = n
        .checked_mul(hours_per_unit)
        .map(|h| h.div_ceil(24))
        .and_then(|d| u32::try_from(d).ok())
        .ok_or_else(|| anyhow::anyhow!("'{s}' is too long ago"))?;
    Ok(days)
}

fn question(plan: &PrunePlan) -> String {
    let n = plural(plan.runs.len(), "run", "runs");
    let size = format_bytes(plan.bytes);
    match plan.mode {
        PruneMode::Wipe => format!("Wipe ALL AI data ({size}) and reset the audit chain? [y/N] "),
        PruneMode::Archive => format!("Archive and prune {n} ({size})? [y/N] "),
        PruneMode::Delete => format!("Prune {n} ({size})? [y/N] "),
    }
}

fn confirm(question: &str) -> Result<bool> {
    print!("{question}");
    std::io::stdout().flush()?;
    let mut answer = String::new();
    std::io::stdin().read_line(&mut answer)?;
    Ok(matches!(answer.trim(), "y" | "Y" | "yes" | "Yes"))
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// The plan as a table of runs and a summary line.
fn render_plan(style: &Style, plan: &PrunePlan) -> String {
    use std::fmt::Write;
    let mut o = String::new();
    let verb = match plan.mode {
        PruneMode::Delete => "Prune",
        PruneMode::Archive => "Archive and prune",
        PruneMode::Wipe => "Wipe",
    };
    if plan.runs.is_empty() {
        let _ = writeln!(o, "No ended runs match: nothing to prune.");
    } else {
        let _ = writeln!(
            o,
            "{}",
            style.paint(
                Tone::Strong,
                &format!(
                    "{verb} {}: {}, {}, about {}",
                    plural(plan.runs.len(), "run", "runs"),
                    plural(plan.entries as usize, "log entry", "log entries"),
                    plural(plan.recordings as usize, "recording", "recordings"),
                    format_bytes(plan.bytes)
                )
            )
        );
        let now = chrono::Utc::now();
        let w_run = 8;
        let w_agent = plan
            .runs
            .iter()
            .map(|r| ui::display_width(r.client.as_deref().unwrap_or("-")))
            .max()
            .unwrap_or(0)
            .max("AGENT".len());
        let w_started = plan
            .runs
            .iter()
            .map(|r| {
                ui::display_width(
                    &r.started
                        .as_deref()
                        .map(|s| ui::day_clock(s, now))
                        .unwrap_or_else(|| "-".into()),
                )
            })
            .max()
            .unwrap_or(0)
            .max("STARTED".len());
        let header = format!(
            "{}   {}   {}   {:>7}   {:>10}   {:>9}",
            ui::pad("RUN", w_run),
            ui::pad("AGENT", w_agent),
            ui::pad("STARTED", w_started),
            "ENTRIES",
            "RECORDINGS",
            "BACKUPS"
        );
        let _ = writeln!(o, "{}", style.paint(Tone::Dim, &header));
        for r in &plan.runs {
            let started = r
                .started
                .as_deref()
                .map(|s| ui::day_clock(s, now))
                .unwrap_or_else(|| "-".into());
            let _ = writeln!(
                o,
                "{}   {}   {}   {:>7}   {:>10}   {:>9}",
                ui::pad(&ui::short_id(&r.run_id), w_run),
                ui::pad(r.client.as_deref().unwrap_or("-"), w_agent),
                ui::pad(&started, w_started),
                r.entries,
                r.recordings,
                format_bytes(r.backup_bytes)
            );
        }
    }
    if !plan.skipped_active.is_empty() {
        let ids: Vec<String> = plan
            .skipped_active
            .iter()
            .map(|i| ui::short_id(i))
            .collect();
        let _ = writeln!(
            o,
            "{} skipped, still running: {}",
            style.mark(Glyph::Running),
            ids.join(", ")
        );
    }
    if let Some(dir) = &plan.archive_dir {
        let _ = writeln!(o, "Archive: {}", dir.display());
    }
    o
}

fn render_outcome(style: &Style, out: &PruneOutcome) -> String {
    use std::fmt::Write;
    let p = &out.plan;
    let mut o = String::new();
    let _ = writeln!(
        o,
        "{} {} {}, freed about {}; log {} {} {}",
        style.mark(Glyph::Ok),
        match p.mode {
            PruneMode::Wipe => "Wiped",
            _ => "Pruned",
        },
        if p.mode == PruneMode::Wipe {
            "all AI data".to_string()
        } else {
            plural(p.runs.len(), "run", "runs")
        },
        format_bytes(p.bytes),
        format_bytes(p.log_bytes_before),
        style.glyph(Glyph::Arrow),
        format_bytes(out.log_bytes_after),
    );
    if let Some(dir) = &out.archive_dir {
        let _ = writeln!(o, "Archive: {}", dir.display());
    }
    if !p.skipped_active.is_empty() {
        let _ = writeln!(
            o,
            "{} {} left alone: still running",
            style.mark(Glyph::Running),
            plural(p.skipped_active.len(), "run", "runs")
        );
    }
    if out.verify_ok {
        let _ = writeln!(
            o,
            "{} verify: the audit chain is intact",
            style.mark(Glyph::Ok)
        );
    } else {
        let _ = writeln!(
            o,
            "{} verify: the audit chain FAILED",
            style.mark(Glyph::Bad)
        );
    }
    o
}

fn render_storage(style: &Style, s: &AiStorage) -> String {
    use std::fmt::Write;
    let mut o = String::new();
    let rows = [
        ("audit log", s.log_bytes),
        ("recordings", s.recordings_bytes),
        ("backups", s.backups_bytes),
        ("runs database", s.runs_db_bytes),
        ("archives", s.archives_bytes),
    ];
    for (label, bytes) in rows {
        let _ = writeln!(
            o,
            "{}  {}",
            style.paint(Tone::Dim, &ui::pad(label, 13)),
            format_bytes(bytes)
        );
    }
    let oldest = s
        .oldest
        .as_deref()
        .map(|t| format!(", oldest {}", ui::day_clock(t, chrono::Utc::now())))
        .unwrap_or_default();
    let _ = writeln!(o, "{}{oldest}", plural(s.runs as usize, "run", "runs"));
    o
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(Parser)]
    struct Cli {
        #[command(flatten)]
        args: AiArgsPrune,
    }

    fn parse(args: &[&str]) -> Result<PruneRequest> {
        let cli = Cli::try_parse_from(std::iter::once("prune").chain(args.iter().copied()))?;
        request(&cli.args)
    }

    #[test]
    fn ages_round_up_to_days() {
        assert_eq!(parse_age("30d").unwrap(), 30);
        assert_eq!(parse_age("12h").unwrap(), 1);
        assert_eq!(parse_age("25h").unwrap(), 2);
        assert_eq!(parse_age("2w").unwrap(), 14);
        assert_eq!(parse_age("7").unwrap(), 7);
        assert!(parse_age("soon").is_err());
        assert!(parse_age("").is_err());
        assert!(parse_age("99999999999w").is_err());
    }

    #[test]
    fn exactly_one_selection() {
        assert!(parse(&[]).is_err());
        assert!(parse(&["--all", "--older-than", "1d"]).is_err());
        assert!(parse(&["--all", "--run", "x"]).is_err());
        let r = parse(&["--run", "a", "--run", "b"]).unwrap();
        assert_eq!(
            r.selection,
            PruneSelection::Runs {
                ids: vec!["a".into(), "b".into()]
            }
        );
        assert_eq!(r.mode, PruneMode::Delete);
        assert_eq!(r.what, PruneWhat::default());
    }

    #[test]
    fn modes_and_narrowing() {
        let r = parse(&["--all", "--archive", "--keep-backups"]).unwrap();
        assert_eq!(r.mode, PruneMode::Archive);
        assert_eq!(r.archive_dir, None);
        assert!(!r.what.backups && r.what.recordings && r.what.runs_db);
        let r = parse(&["--older-than", "1w", "--archive", "/tmp/x"]).unwrap();
        assert_eq!(r.archive_dir, Some("/tmp/x".into()));
        assert_eq!(parse(&["--all", "--wipe"]).unwrap().mode, PruneMode::Wipe);
        assert!(parse(&["--older-than", "1d", "--wipe"]).is_err());
        assert!(parse(&["--all", "--wipe", "--archive"]).is_err());
        assert!(parse(&["--all", "--wipe", "--keep-runs-db"]).is_err());
    }

    #[test]
    fn plan_text_names_runs_and_skips() {
        let plan = PrunePlan {
            runs: vec![prune::PrunedRun {
                run_id: "0123456789abcdef".into(),
                client: Some("claude-code".into()),
                started: Some("2026-01-02T03:04:05Z".into()),
                ended: None,
                entries: 12,
                recordings: 3,
                backup_bytes: 2048,
            }],
            skipped_active: vec!["fedcba9876543210".into()],
            entries: 12,
            recordings: 3,
            bytes: 4096,
            log_bytes_before: 9000,
            mode: PruneMode::Delete,
            archive_dir: None,
        };
        let out = render_plan(&Style::plain(), &plan);
        assert!(
            out.contains("Prune 1 run: 12 log entries, 3 recordings"),
            "{out}"
        );
        assert!(
            out.contains("01234567") && out.contains("claude-code"),
            "{out}"
        );
        assert!(out.contains("skipped, still running: fedcba98"), "{out}");
        assert_eq!(question(&plan), "Prune 1 run (4.0 KiB)? [y/N] ");
    }
}
