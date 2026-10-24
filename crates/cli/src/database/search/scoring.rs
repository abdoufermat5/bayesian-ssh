use crate::config::AppConfig;
use crate::database::Database;
use crate::models::Connection;
use anyhow::Result;
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// Launch context used at query time for predictive ranking.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RankContext {
    pub git_root: Option<String>,
    pub hour: Option<u8>,
    pub prev_connection_id: Option<Uuid>,
}

impl RankContext {
    pub fn current(db: &Database, cfg: &AppConfig) -> Self {
        let ctx = crate::services::context::capture(db, cfg);
        Self {
            git_root: ctx.git_root,
            hour: ctx.local_hour,
            prev_connection_id: ctx.prev_connection_id,
        }
    }

    pub fn current_without_cwd(db: &Database) -> Self {
        let ctx = crate::services::context::capture_without_cwd(db);
        Self {
            git_root: None,
            hour: ctx.local_hour,
            prev_connection_id: ctx.prev_connection_id,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Prediction {
    pub connection: Connection,
    pub score: f64,
    pub breakdown: ScoreBreakdown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoreBreakdown {
    pub prior: f64,
    pub likelihood: f64,
    pub recency: f64,
    pub success: f64,
    pub repo: Option<f64>,
    pub hour: Option<f64>,
    pub after_prev: Option<f64>,
    pub context_factor: f64,
    pub repo_hits: i64,
    pub hour_hits: i64,
    pub after_prev_hits: i64,
    pub total_w: i64,
}

impl ScoreBreakdown {
    pub fn score(&self) -> f64 {
        self.prior * self.likelihood * self.recency * self.success * self.context_factor
    }
}

impl Prediction {
    /// Build a one-line human-readable reason from the strongest context feature,
    /// falling back to usage count and recency when no context feature applies.
    pub fn format_reason(&self, ctx: &RankContext, prev_name: Option<&str>) -> String {
        let b = &self.breakdown;
        let mut best: Option<(&str, f64)> = None;
        for (name, lr_opt, hits) in [
            ("repo", b.repo, b.repo_hits),
            ("after_prev", b.after_prev, b.after_prev_hits),
            ("hour", b.hour, b.hour_hits),
        ] {
            if let Some(lr) = lr_opt {
                if lr > 1.0 && hits > 0 && best.is_none_or(|(_, best_lr)| lr > best_lr) {
                    best = Some((name, lr));
                }
            }
        }

        if let Some((feature, _)) = best {
            match feature {
                "repo" => {
                    if let Some(ref root) = ctx.git_root {
                        let abbr = crate::services::context::abbreviate_home(root);
                        return format!(
                            "{}/{} of your sessions from {abbr}",
                            b.repo_hits, b.total_w
                        );
                    }
                }
                "after_prev" => {
                    if let Some(prev) = prev_name {
                        return format!("usually opened right after {prev}");
                    }
                }
                "hour" => {
                    if let Some(h) = ctx.hour {
                        return format!("used often around {h:02}:00");
                    }
                }
                _ => {}
            }
        }

        let times_word = if b.total_w == 1 { "time" } else { "times" };
        match (b.total_w > 0, self.connection.last_used) {
            (true, Some(last)) => format!(
                "used {} {}, last {}",
                b.total_w,
                times_word,
                crate::cli::utils::format_duration(last)
            ),
            (true, None) => format!("used {} {}", b.total_w, times_word),
            (false, Some(last)) => format!("last {}", crate::cli::utils::format_duration(last)),
            (false, None) => "never used".to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct FeatureCounts {
    pub hits: i64,
    pub global: i64,
}

/// Compute the clamped product of smoothed likelihood ratios across `{repo, hour, after_prev}`.
///
/// Each feature `f` is used only when `Some` (current context value is known) and `global >= 3`.
pub(crate) fn context_factor(
    features: &[Option<FeatureCounts>],
    total_w: i64,
    n: i64,
) -> (f64, [Option<f64>; 3]) {
    const ALPHA: f64 = 1.0;
    let mut lrs = [None; 3];
    let mut product = 1.0;
    let mut any_used = false;
    let total_w_f = total_w.max(0) as f64;
    let n_f = n.max(0) as f64;

    for (idx, feat_opt) in features.iter().take(3).enumerate() {
        if let Some(fc) = feat_opt {
            if fc.global >= 3 {
                let p_conn = (fc.hits.max(0) as f64 + ALPHA) / (total_w_f + 2.0 * ALPHA);
                let p_global =
                    (fc.global as f64 + ALPHA) / (n_f.max(fc.global as f64) + 2.0 * ALPHA);
                let lr = p_conn / p_global;
                lrs[idx] = Some(lr);
                product *= lr;
                any_used = true;
            }
        }
    }

    let factor = if any_used {
        product.clamp(0.25, 8.0)
    } else {
        1.0
    };
    (factor, lrs)
}

fn neighbour_hours(hour: Option<u8>) -> [i64; 3] {
    match hour {
        Some(h) => {
            let h = (h % 24) as i64;
            [(h + 23) % 24, h, (h + 1) % 24]
        }
        None => [-1, -1, -1],
    }
}

impl Database {
    /// Rank all non-stale connections with no query (`likelihood = 1.0`) in the given context.
    pub fn predict(&self, ctx: &RankContext, limit: usize) -> Result<Vec<Prediction>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let mut connections = self.list_connections(None, false)?;
        connections.retain(|c| c.stale_since.is_none());
        if connections.is_empty() {
            return Ok(Vec::new());
        }

        let indexed: Vec<(Connection, String, String)> = connections
            .into_iter()
            .map(|c| {
                let name_lower = c.name.to_lowercase();
                let host_lower = c.host.to_lowercase();
                (c, name_lower, host_lower)
            })
            .collect();

        let cutoff = self.window_cutoff_rfc3339();
        let aggregates = self.session_aggregates(&indexed, Some(ctx), &cutoff);
        let totals = self.global_window_totals(Some(ctx), &cutoff);

        let mut predictions: Vec<Prediction> = indexed
            .into_iter()
            .map(|(conn, name_lower, host_lower)| {
                let breakdown = self.calculate_bayesian_score(
                    &conn,
                    "",
                    &name_lower,
                    &host_lower,
                    aggregates.get(&conn.id).copied(),
                    Some(ctx),
                    totals,
                );
                let score = breakdown.score();
                Prediction {
                    connection: conn,
                    score,
                    breakdown,
                }
            })
            .collect();

        predictions.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.connection.name.cmp(&b.connection.name))
        });
        predictions.truncate(limit);
        Ok(predictions)
    }

    /// Explain the score and rank of `connection` in `ctx` across all active connections.
    pub fn explain_connection(
        &self,
        connection: &Connection,
        ctx: &RankContext,
    ) -> Result<(Prediction, usize, usize)> {
        let all = self.predict(ctx, usize::MAX)?;
        if let Some(idx) = all.iter().position(|p| p.connection.id == connection.id) {
            let total = all.len();
            return Ok((all[idx].clone(), idx + 1, total));
        }

        let name_lower = connection.name.to_lowercase();
        let host_lower = connection.host.to_lowercase();
        let indexed = vec![(connection.clone(), name_lower.clone(), host_lower.clone())];
        let cutoff = self.window_cutoff_rfc3339();
        let aggregates = self.session_aggregates(&indexed, Some(ctx), &cutoff);
        let totals = self.global_window_totals(Some(ctx), &cutoff);
        let breakdown = self.calculate_bayesian_score(
            connection,
            "",
            &name_lower,
            &host_lower,
            aggregates.get(&connection.id).copied(),
            Some(ctx),
            totals,
        );
        let score = breakdown.score();
        let rank = all.iter().filter(|p| p.score > score).count() + 1;
        let total = all.len() + 1;
        Ok((
            Prediction {
                connection: connection.clone(),
                score,
                breakdown,
            },
            rank,
            total,
        ))
    }

    pub(super) fn deduplicate_and_rank(
        &self,
        connections: &mut Vec<Connection>,
        query: &str,
        mode: &str,
        ctx: Option<&RankContext>,
    ) {
        use std::collections::HashSet;

        let mut seen = HashSet::new();
        connections.retain(|conn| {
            if seen.contains(&conn.id) {
                false
            } else {
                seen.insert(conn.id);
                true
            }
        });

        let query_lower = query.to_lowercase();

        // Precompute per-connection lowercase name AND host once
        // (not once per comparison).
        let mut indexed: Vec<(Connection, String, String)> = connections
            .drain(..)
            .map(|c| {
                let name_lower = c.name.to_lowercase();
                let host_lower = c.host.to_lowercase();
                (c, name_lower, host_lower)
            })
            .collect();

        let cutoff = self.window_cutoff_rfc3339();
        // Pull aggregate usage statistics in ONE query rather than 2*N.
        let aggregates = self.session_aggregates(&indexed, ctx, &cutoff);
        let totals = self.global_window_totals(ctx, &cutoff);

        indexed.sort_by(
            |(a, a_name_lower, a_host_lower), (b, b_name_lower, b_host_lower)| {
                let score_a = if mode == "bayesian" {
                    self.calculate_bayesian_score(
                        a,
                        &query_lower,
                        a_name_lower,
                        a_host_lower,
                        aggregates.get(&a.id).copied(),
                        ctx,
                        totals,
                    )
                    .score()
                } else {
                    self.calculate_relevance_score(a, &query_lower, a_name_lower, a_host_lower)
                };
                let score_b = if mode == "bayesian" {
                    self.calculate_bayesian_score(
                        b,
                        &query_lower,
                        b_name_lower,
                        b_host_lower,
                        aggregates.get(&b.id).copied(),
                        ctx,
                        totals,
                    )
                    .score()
                } else {
                    self.calculate_relevance_score(b, &query_lower, b_name_lower, b_host_lower)
                };
                score_b
                    .partial_cmp(&score_a)
                    .unwrap_or(std::cmp::Ordering::Equal)
            },
        );

        *connections = indexed.into_iter().map(|(c, _, _)| c).collect();
    }

    fn window_cutoff_rfc3339(&self) -> String {
        let days = i64::from(self.window_days.max(1));
        Utc::now()
            .checked_sub_signed(Duration::days(days))
            .unwrap_or_else(Utc::now)
            .to_rfc3339()
    }

    /// Fetch all-time `(total, successes)` and windowed context hit counts for a
    /// set of connection IDs in a single `GROUP BY` query.
    fn session_aggregates(
        &self,
        connections: &[(Connection, String, String)],
        ctx: Option<&RankContext>,
        cutoff: &str,
    ) -> HashMap<Uuid, SessionAggregate> {
        let mut out = HashMap::new();
        if connections.is_empty() {
            return out;
        }
        let ids: Vec<String> = connections
            .iter()
            .map(|(c, _, _)| c.id.to_string())
            .collect();
        let placeholders = vec!["?"; ids.len()].join(",");
        let sql = format!(
            "SELECT connection_id,
                    COUNT(*) AS total,
                    COALESCE(SUM(CASE WHEN exit_code = 0 OR exit_code IS NULL THEN 1 ELSE 0 END), 0) AS successes,
                    COALESCE(SUM(CASE WHEN started_at >= ? THEN 1 ELSE 0 END), 0) AS total_w,
                    COALESCE(SUM(CASE WHEN started_at >= ? AND git_root = ? THEN 1 ELSE 0 END), 0) AS repo_hits,
                    COALESCE(SUM(CASE WHEN started_at >= ? AND local_hour IN (?, ?, ?) THEN 1 ELSE 0 END), 0) AS hour_hits,
                    COALESCE(SUM(CASE WHEN started_at >= ? AND prev_connection_id = ? THEN 1 ELSE 0 END), 0) AS after_prev_hits
             FROM sessions
             WHERE connection_id IN ({placeholders})
             GROUP BY connection_id"
        );

        let git_root: Option<&str> = ctx.and_then(|c| c.git_root.as_deref());
        let [h0, h1, h2] = neighbour_hours(ctx.and_then(|c| c.hour));
        let prev_id: Option<String> =
            ctx.and_then(|c| c.prev_connection_id.map(|id| id.to_string()));

        let mut sql_params: Vec<&dyn rusqlite::ToSql> = vec![
            &cutoff, &cutoff, &git_root, &cutoff, &h0, &h1, &h2, &cutoff, &prev_id,
        ];
        for id in &ids {
            sql_params.push(id as &dyn rusqlite::ToSql);
        }

        let mut stmt = match self.conn.prepare(&sql) {
            Ok(s) => s,
            Err(_) => return out,
        };
        let mut rows = match stmt.query(rusqlite::params_from_iter(sql_params.iter())) {
            Ok(r) => r,
            Err(_) => return out,
        };
        while let Ok(Some(row)) = rows.next() {
            let conn_id_str: String = row.get(0).unwrap_or_default();
            if let Ok(conn_id) = Uuid::parse_str(&conn_id_str) {
                out.insert(
                    conn_id,
                    SessionAggregate {
                        total: row.get(1).unwrap_or(0),
                        successes: row.get(2).unwrap_or(0),
                        total_w: row.get(3).unwrap_or(0),
                        repo_hits: row.get(4).unwrap_or(0),
                        hour_hits: row.get(5).unwrap_or(0),
                        after_prev_hits: row.get(6).unwrap_or(0),
                    },
                );
            }
        }
        out
    }

    /// Fetch global session totals (`N`, `N_repo`, `N_hour`, `N_prev`) over the window in one query.
    fn global_window_totals(&self, ctx: Option<&RankContext>, cutoff: &str) -> GlobalTotals {
        let Some(ctx) = ctx else {
            return GlobalTotals::default();
        };
        let git_root: Option<&str> = ctx.git_root.as_deref();
        let [h0, h1, h2] = neighbour_hours(ctx.hour);
        let prev_id: Option<String> = ctx.prev_connection_id.map(|id| id.to_string());

        self.conn
            .query_row(
                "SELECT COUNT(*) AS n,
                        COALESCE(SUM(CASE WHEN git_root = ? THEN 1 ELSE 0 END), 0) AS n_repo,
                        COALESCE(SUM(CASE WHEN local_hour IN (?, ?, ?) THEN 1 ELSE 0 END), 0) AS n_hour,
                        COALESCE(SUM(CASE WHEN prev_connection_id = ? THEN 1 ELSE 0 END), 0) AS n_prev
                 FROM sessions
                 WHERE started_at >= ? AND connection_id IS NOT NULL",
                rusqlite::params![git_root, h0, h1, h2, prev_id, cutoff],
                |row| {
                    Ok(GlobalTotals {
                        n: row.get(0).unwrap_or(0),
                        n_repo: row.get(1).unwrap_or(0),
                        n_hour: row.get(2).unwrap_or(0),
                        n_prev: row.get(3).unwrap_or(0),
                    })
                },
            )
            .unwrap_or_default()
    }

    /// Bayesian-inspired scoring: prior (frequency) × likelihood (match
    /// quality) × recency (temporal decay) × success × context factor.
    #[allow(clippy::too_many_arguments)]
    fn calculate_bayesian_score(
        &self,
        connection: &Connection,
        query_lower: &str,
        name_lower: &str,
        host_lower: &str,
        aggregate: Option<SessionAggregate>,
        ctx: Option<&RankContext>,
        totals: GlobalTotals,
    ) -> ScoreBreakdown {
        // Total session count across all connections is used as a
        // smoothing constant. We use a small fixed denominator so the
        // prior remains stable for new connections.
        let total_connections: f64 = 1.0;
        let agg = aggregate.unwrap_or_default();
        let prior = if total_connections > 0.0 {
            (agg.total as f64 + 1.0) / (total_connections + 10.0) // Laplace smoothing
        } else {
            0.1
        };

        let likelihood =
            self.calculate_match_likelihood(connection, query_lower, name_lower, host_lower);
        let recency = self.calculate_recency_factor(connection);
        let success_rate = if agg.total > 0 {
            agg.successes as f64 / agg.total as f64
        } else {
            0.8
        };
        let success = 0.5 + success_rate * 0.5;

        let features = [
            ctx.and_then(|c| c.git_root.as_ref())
                .map(|_| FeatureCounts {
                    hits: agg.repo_hits,
                    global: totals.n_repo,
                }),
            ctx.and_then(|c| c.hour).map(|_| FeatureCounts {
                hits: agg.hour_hits,
                global: totals.n_hour,
            }),
            ctx.and_then(|c| c.prev_connection_id)
                .map(|_| FeatureCounts {
                    hits: agg.after_prev_hits,
                    global: totals.n_prev,
                }),
        ];
        let (ctx_factor, [repo, hour, after_prev]) =
            context_factor(&features, agg.total_w, totals.n);

        ScoreBreakdown {
            prior,
            likelihood,
            recency,
            success,
            repo,
            hour,
            after_prev,
            context_factor: ctx_factor,
            repo_hits: agg.repo_hits,
            hour_hits: agg.hour_hits,
            after_prev_hits: agg.after_prev_hits,
            total_w: agg.total_w,
        }
    }

    fn calculate_match_likelihood(
        &self,
        connection: &Connection,
        query_lower: &str,
        name_lower: &str,
        host_lower: &str,
    ) -> f64 {
        if query_lower.is_empty() || name_lower == query_lower {
            return 1.0;
        }
        if name_lower.starts_with(query_lower) {
            return 0.9;
        }

        let words: Vec<&str> = name_lower.split(&['-', '_', '.', ' '][..]).collect();
        for word in &words {
            if *word == query_lower {
                return 0.85;
            }
            if word.starts_with(query_lower) {
                return 0.75;
            }
        }

        if name_lower.contains(query_lower) {
            return 0.6;
        }

        let normalized_name = name_lower.replace(&['-', '_', '.'][..], "");
        let normalized_query = query_lower.replace(&['-', '_', '.'][..], "");
        if normalized_name.contains(&normalized_query) {
            return 0.5;
        }

        if host_lower.contains(query_lower) {
            return 0.4;
        }

        for tag in &connection.tags {
            if tag.to_lowercase().contains(query_lower) {
                return 0.45;
            }
        }

        if query_lower.len() >= 2 && words.len() > 1 {
            let acronym: String = words.iter().filter_map(|w| w.chars().next()).collect();
            if acronym.contains(query_lower) {
                return 0.35;
            }
        }

        if self.matches_enhanced_patterns(query_lower, name_lower) {
            return 0.2;
        }
        0.1
    }

    fn calculate_recency_factor(&self, connection: &Connection) -> f64 {
        if let Some(last_used) = connection.last_used {
            let hours_since_used = Utc::now().signed_duration_since(last_used).num_hours() as f64;
            let lambda = 0.005;
            let decay = (-lambda * hours_since_used).exp();
            decay.max(0.1)
        } else {
            0.3
        }
    }

    fn calculate_relevance_score(
        &self,
        connection: &Connection,
        query_lower: &str,
        name_lower: &str,
        host_lower: &str,
    ) -> f64 {
        let mut score = 0.0;

        if name_lower == query_lower {
            score += 100.0;
        }
        if name_lower.starts_with(query_lower) {
            score += 50.0;
        }
        if name_lower.contains(query_lower) {
            score += 25.0;
        }
        if self.matches_enhanced_patterns(query_lower, name_lower) {
            score += 15.0;
        }
        if host_lower.contains(query_lower) {
            score += 15.0;
        }
        for tag in &connection.tags {
            if tag.to_lowercase().contains(query_lower) {
                score += 20.0;
                break;
            }
        }

        if let Some(last_used) = connection.last_used {
            let hours_since_used = Utc::now().signed_duration_since(last_used).num_hours();

            if hours_since_used < 24 {
                score += 30.0;
            } else if hours_since_used < 168 {
                score += 15.0;
            } else if hours_since_used < 720 {
                score += 5.0;
            }
        }

        score
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct SessionAggregate {
    total: i64,
    successes: i64,
    total_w: i64,
    repo_hits: i64,
    hour_hits: i64,
    after_prev_hits: i64,
}

#[derive(Debug, Clone, Copy, Default)]
struct GlobalTotals {
    n: i64,
    n_repo: i64,
    n_hour: i64,
    n_prev: i64,
}

#[cfg(test)]
mod tests {
    use super::{context_factor, FeatureCounts, RankContext};
    use crate::config::AppConfig;
    use crate::database::Database;
    use crate::models::{Connection, Session, SessionContext};
    use tempfile::tempdir;

    #[test]
    fn predictive_context_factor() {
        // No usable feature -> 1.0
        let (factor, lrs) = context_factor(&[None, None, None], 10, 20);
        assert_eq!(factor, 1.0);
        assert_eq!(lrs, [None, None, None]);

        // Feature with global < 3 is ignored -> 1.0
        let (factor, lrs) = context_factor(
            &[Some(FeatureCounts { hits: 2, global: 2 }), None, None],
            5,
            10,
        );
        assert_eq!(factor, 1.0);
        assert_eq!(lrs, [None, None, None]);

        // Strong positive feature raises factor and clamps at 8.0
        let (factor_high, lrs_high) = context_factor(
            &[
                Some(FeatureCounts {
                    hits: 10,
                    global: 10,
                }),
                Some(FeatureCounts {
                    hits: 10,
                    global: 10,
                }),
                None,
            ],
            10,
            100,
        );
        assert!(lrs_high[0].unwrap() > 1.0);
        assert_eq!(factor_high, 8.0);

        // Strong negative evidence clamps at 0.25
        let (factor_low, _) = context_factor(
            &[
                Some(FeatureCounts {
                    hits: 0,
                    global: 50,
                }),
                None,
                None,
            ],
            20,
            55,
        );
        assert_eq!(factor_low, 0.25);

        // Connection with 0 sessions stays finite
        let (factor_zero, lrs_zero) = context_factor(
            &[Some(FeatureCounts { hits: 0, global: 5 }), None, None],
            0,
            10,
        );
        assert!(factor_zero.is_finite());
        assert!(lrs_zero[0].unwrap().is_finite());
    }

    #[test]
    fn predictive_ranking_by_git_root() {
        let dir = tempdir().unwrap();
        let config = AppConfig {
            database_path: dir.path().join("test.db"),
            ..Default::default()
        };
        let db = Database::new(&config).unwrap();

        let conn_a = Connection::new(
            "host-a".into(),
            "a.example.com".into(),
            "root".into(),
            22,
            None,
            None,
            false,
            None,
        );
        let conn_b = Connection::new(
            "host-b".into(),
            "b.example.com".into(),
            "root".into(),
            22,
            None,
            None,
            false,
            None,
        );
        db.add_connection(&conn_a).unwrap();
        db.add_connection(&conn_b).unwrap();

        for _ in 0..5 {
            let mut s_a = Session::new(conn_a.clone());
            s_a.context = SessionContext {
                git_root: Some("/r/a".into()),
                ..Default::default()
            };
            s_a.mark_terminated(0);
            db.add_session(&s_a).unwrap();

            let mut s_b = Session::new(conn_b.clone());
            s_b.context = SessionContext {
                git_root: Some("/r/b".into()),
                ..Default::default()
            };
            s_b.mark_terminated(0);
            db.add_session(&s_b).unwrap();
        }

        let preds_a = db
            .predict(
                &RankContext {
                    git_root: Some("/r/a".into()),
                    hour: None,
                    prev_connection_id: None,
                },
                10,
            )
            .unwrap();
        assert_eq!(preds_a[0].connection.name, "host-a");
        assert_eq!(preds_a[1].connection.name, "host-b");

        let preds_b = db
            .predict(
                &RankContext {
                    git_root: Some("/r/b".into()),
                    hour: None,
                    prev_connection_id: None,
                },
                10,
            )
            .unwrap();
        assert_eq!(preds_b[0].connection.name, "host-b");
        assert_eq!(preds_b[1].connection.name, "host-a");
    }
}
