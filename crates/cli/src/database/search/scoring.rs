use crate::database::Database;
use crate::models::Connection;
use std::collections::HashMap;

impl Database {
    pub(super) fn deduplicate_and_rank(
        &self,
        connections: &mut Vec<Connection>,
        query: &str,
        mode: &str,
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

        // Pull aggregate usage statistics in ONE query rather than 2*N.
        let aggregates = self.session_aggregates(&indexed);

        indexed.sort_by(
            |(a, a_name_lower, a_host_lower), (b, b_name_lower, b_host_lower)| {
                let score_a = if mode == "bayesian" {
                    self.calculate_bayesian_score(
                        a,
                        &query_lower,
                        a_name_lower,
                        a_host_lower,
                        aggregates.get(&a.id).copied(),
                    )
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
                    )
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

    /// Fetch `(total_sessions, success_count)` for a set of connection IDs in
    /// a single GROUP BY query. Connections that have no sessions are
    /// absent from the result; callers must default to (0, 0) for those.
    fn session_aggregates(
        &self,
        connections: &[(Connection, String, String)],
    ) -> HashMap<uuid::Uuid, SessionAggregate> {
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
                    COALESCE(SUM(CASE WHEN exit_code = 0 OR exit_code IS NULL THEN 1 ELSE 0 END), 0) AS successes
             FROM sessions
             WHERE connection_id IN ({placeholders})
             GROUP BY connection_id"
        );
        let sql_params: Vec<&dyn rusqlite::ToSql> =
            ids.iter().map(|id| id as &dyn rusqlite::ToSql).collect();
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
            if let Ok(conn_id) = uuid::Uuid::parse_str(&conn_id_str) {
                let total: i64 = row.get(1).unwrap_or(0);
                let successes: i64 = row.get(2).unwrap_or(0);
                out.insert(conn_id, SessionAggregate { total, successes });
            }
        }
        out
    }

    /// Bayesian-inspired scoring: prior (frequency) × likelihood (match
    /// quality) × recency (temporal decay) × success rate.
    fn calculate_bayesian_score(
        &self,
        connection: &Connection,
        query_lower: &str,
        name_lower: &str,
        host_lower: &str,
        aggregate: Option<SessionAggregate>,
    ) -> f64 {
        // Total session count across all connections is used as a
        // smoothing constant. We use a small fixed denominator so the
        // prior remains stable for new connections.
        let total_connections: f64 = 1.0;
        let agg = aggregate.unwrap_or(SessionAggregate {
            total: 0,
            successes: 0,
        });
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

        let score = prior * likelihood * recency * (0.5 + success_rate * 0.5);
        score * 100.0
    }

    fn calculate_match_likelihood(
        &self,
        connection: &Connection,
        query_lower: &str,
        name_lower: &str,
        host_lower: &str,
    ) -> f64 {
        if name_lower == query_lower {
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
            let hours_since_used = chrono::Utc::now()
                .signed_duration_since(last_used)
                .num_hours() as f64;
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
            let hours_since_used = chrono::Utc::now()
                .signed_duration_since(last_used)
                .num_hours();

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

#[derive(Debug, Clone, Copy)]
struct SessionAggregate {
    total: i64,
    successes: i64,
}
