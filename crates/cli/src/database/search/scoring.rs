use crate::database::Database;
use crate::models::Connection;
use rusqlite::params;

impl Database {
    pub(super) fn deduplicate_and_rank(&self, connections: &mut Vec<Connection>, query: &str, mode: &str) {
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

        // Precompute per-connection lowercase name once (not once per comparison).
        let mut indexed: Vec<(Connection, String)> = connections
            .drain(..)
            .map(|c| {
                let name_lower = c.name.to_lowercase();
                (c, name_lower)
            })
            .collect();

        // Sort by relevance score based on mode
        indexed.sort_by(|(a, a_name_lower), (b, b_name_lower)| {
            let score_a = if mode == "bayesian" {
                self.calculate_bayesian_score(a, &query_lower, a_name_lower)
            } else {
                self.calculate_relevance_score(a, &query_lower, a_name_lower)
            };
            let score_b = if mode == "bayesian" {
                self.calculate_bayesian_score(b, &query_lower, b_name_lower)
            } else {
                self.calculate_relevance_score(b, &query_lower, b_name_lower)
            };
            score_b
                .partial_cmp(&score_a)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        *connections = indexed.into_iter().map(|(c, _)| c).collect();
    }

    /// Bayesian-inspired scoring that combines:
    /// - Prior probability (frequency of use)
    /// - Likelihood (match quality)
    /// - Recency (temporal decay)
    fn calculate_bayesian_score(
        &self,
        connection: &Connection,
        query_lower: &str,
        name_lower: &str,
    ) -> f64 {
        // Get connection usage statistics
        let (frequency, total_connections) = self.get_connection_frequency(&connection.id);

        // 1. Prior probability: P(connection) based on historical usage
        let prior = if total_connections > 0 {
            (frequency as f64 + 1.0) / (total_connections as f64 + 10.0) // Laplace smoothing
        } else {
            0.1 // Default prior for new connections
        };

        // 2. Likelihood: P(query | connection) - how well does query match?
        let likelihood = self.calculate_match_likelihood(connection, query_lower, name_lower);

        // 3. Recency factor: exponential decay based on last use
        let recency = self.calculate_recency_factor(connection);

        // 4. Success rate bonus
        let success_rate = self.get_connection_success_rate(&connection.id);

        // Combine using Bayesian-inspired formula:
        // Score = Prior * Likelihood * Recency * SuccessBonus
        let score = prior * likelihood * recency * (0.5 + success_rate * 0.5);

        // Scale to readable range
        score * 100.0
    }

    fn get_connection_frequency(&self, connection_id: &uuid::Uuid) -> (i64, i64) {
        let conn_count: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM sessions WHERE connection_id = ?",
                params![connection_id.to_string()],
                |row| row.get(0),
            )
            .unwrap_or(0);

        let total: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))
            .unwrap_or(1);

        (conn_count, total)
    }

    fn get_connection_success_rate(&self, connection_id: &uuid::Uuid) -> f64 {
        let result: Result<(i64, i64), _> = self.conn.query_row(
            "SELECT 
                COUNT(CASE WHEN exit_code = 0 OR exit_code IS NULL THEN 1 END),
                COUNT(*)
             FROM sessions WHERE connection_id = ?",
            params![connection_id.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        );

        match result {
            Ok((successes, total)) if total > 0 => successes as f64 / total as f64,
            _ => 0.8, // Default success rate for new connections
        }
    }

    fn calculate_match_likelihood(
        &self,
        connection: &Connection,
        query_lower: &str,
        name_lower: &str,
    ) -> f64 {
        // Exact match - highest likelihood
        if name_lower == query_lower {
            return 1.0;
        }

        // Prefix match - very high
        if name_lower.starts_with(&query_lower) {
            return 0.9;
        }

        // Word boundary match (e.g., "prod" matches "web-prod-server")
        let words: Vec<&str> = name_lower.split(&['-', '_', '.', ' '][..]).collect();
        for word in &words {
            if *word == query_lower {
                return 0.85;
            }
            if word.starts_with(&query_lower) {
                return 0.75;
            }
        }

        // Contains match
        if name_lower.contains(&query_lower) {
            return 0.6;
        }

        // Normalized match (ignoring separators)
        let normalized_name = name_lower.replace(&['-', '_', '.'][..], "");
        let normalized_query = query_lower.replace(&['-', '_', '.'][..], "");
        if normalized_name.contains(&normalized_query) {
            return 0.5;
        }

        // Host match
        if connection.host.to_lowercase().contains(&query_lower) {
            return 0.4;
        }

        // Tag match
        for tag in &connection.tags {
            if tag.to_lowercase().contains(&query_lower) {
                return 0.45;
            }
        }

        // Acronym match
        if query_lower.len() >= 2 && words.len() > 1 {
            let acronym: String = words.iter().filter_map(|w| w.chars().next()).collect();
            if acronym.contains(&query_lower) {
                return 0.35;
            }
        }

        // Fuzzy/pattern match (lowest but still valid)
        if self.matches_enhanced_patterns(&query_lower, &name_lower) {
            return 0.2;
        }

        0.1 // Minimal likelihood for any match
    }

    fn calculate_recency_factor(&self, connection: &Connection) -> f64 {
        if let Some(last_used) = connection.last_used {
            let hours_since_used = chrono::Utc::now()
                .signed_duration_since(last_used)
                .num_hours() as f64;

            // Exponential decay: e^(-λt) where λ controls decay rate
            // λ = 0.005 means ~37% weight after 200 hours (~8 days)
            let lambda = 0.005;
            let decay = (-lambda * hours_since_used).exp();

            // Ensure minimum factor of 0.1 for very old connections
            decay.max(0.1)
        } else {
            0.3 // Never used - moderate recency factor
        }
    }

    fn calculate_relevance_score(
        &self,
        connection: &Connection,
        query_lower: &str,
        name_lower: &str,
    ) -> f64 {
        let mut score = 0.0;

        // Exact match in name gets highest score
        if name_lower == query_lower {
            score += 100.0;
        }

        // Starts with query (high relevance)
        if name_lower.starts_with(&query_lower) {
            score += 50.0;
        }

        // Contains query in name
        if name_lower.contains(&query_lower) {
            score += 25.0;
        }

        // Enhanced pattern matching scores
        if self.matches_enhanced_patterns(query_lower, name_lower) {
            score += 15.0; // Bonus for pattern matching
        }

        // Query in host
        if connection.host.to_lowercase().contains(&query_lower) {
            score += 15.0;
        }

        // Query in tags
        for tag in &connection.tags {
            if tag.to_lowercase().contains(&query_lower) {
                score += 20.0;
                break;
            }
        }

        // Recent usage bonus
        if let Some(last_used) = connection.last_used {
            let hours_since_used = chrono::Utc::now()
                .signed_duration_since(last_used)
                .num_hours();

            if hours_since_used < 24 {
                score += 30.0;
            } else if hours_since_used < 168 {
                // 1 week
                score += 15.0;
            } else if hours_since_used < 720 {
                // 1 month
                score += 5.0;
            }
        }

        score
    }
}
