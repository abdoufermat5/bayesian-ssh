use crate::database::Database;
use crate::models::Connection;
use anyhow::Result;
use rusqlite::params;

impl Database {
    // Fuzzy search methods for enhanced connection discovery
    #[allow(dead_code)]
    pub fn fuzzy_search_connections(&self, query: &str, limit: usize) -> Result<Vec<Connection>> {
        self.search_connections(query, limit, "fuzzy")
    }

    // Bayesian search - combines frequency, recency, and match quality
    #[allow(dead_code)]
    pub fn bayesian_search_connections(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<Connection>> {
        self.search_connections(query, limit, "bayesian")
    }

    // Unified search method with mode selection
    pub fn search_connections(
        &self,
        query: &str,
        limit: usize,
        mode: &str,
    ) -> Result<Vec<Connection>> {
        let mut all_matches = Vec::new();
        let normalized_query = query.to_lowercase();

        // Search in names with multiple strategies
        if let Ok(mut name_matches) = self.search_by_field(&normalized_query, "name", limit) {
            all_matches.append(&mut name_matches);
        }

        // Enhanced fuzzy matching for names
        if let Ok(mut fuzzy_matches) = self.enhanced_fuzzy_search(&normalized_query, limit) {
            all_matches.append(&mut fuzzy_matches);
        }

        // Search in hosts
        if let Ok(mut host_matches) = self.search_by_field(&normalized_query, "host", limit) {
            all_matches.append(&mut host_matches);
        }

        // Search in tags (JSON array search)
        if let Ok(mut tag_matches) = self.search_in_tags(&normalized_query, limit) {
            all_matches.append(&mut tag_matches);
        }

        // Remove duplicates and sort by relevance
        self.deduplicate_and_rank(&mut all_matches, &normalized_query, mode);

        // Limit results
        all_matches.truncate(limit);

        Ok(all_matches)
    }

    fn search_by_field(&self, query: &str, field: &str, limit: usize) -> Result<Vec<Connection>> {
        let sql = format!(
            "SELECT id, name, host, user, port, bastion, bastion_user, use_kerberos, key_path, created_at, last_used, tags
             FROM connections
             WHERE {} LIKE ? COLLATE NOCASE
             ORDER BY last_used DESC NULLS LAST, name ASC
             LIMIT ?",
            field
        );

        let mut stmt = self.conn.prepare(&sql)?;
        let like_pattern = format!("%{}%", query);
        let mut rows = stmt.query(params![like_pattern, limit])?;

        let mut connections = Vec::new();
        while let Some(row) = rows.next()? {
            connections.push(self.row_to_connection(row)?);
        }

        Ok(connections)
    }

    fn enhanced_fuzzy_search(&self, query: &str, limit: usize) -> Result<Vec<Connection>> {
        let sql = "SELECT id, name, host, user, port, bastion, bastion_user, use_kerberos, key_path, created_at, last_used, tags
                   FROM connections
                   ORDER BY last_used DESC NULLS LAST, name ASC";

        let mut stmt = self.conn.prepare(sql)?;
        let mut rows = stmt.query([])?;

        let mut connections = Vec::new();
        while let Some(row) = rows.next()? {
            let connection = self.row_to_connection(row)?;
            let name_lower = connection.name.to_lowercase();

            // Enhanced matching patterns
            if self.matches_enhanced_patterns(query, &name_lower) {
                connections.push(connection);
                if connections.len() >= limit {
                    break;
                }
            }
        }

        Ok(connections)
    }

    fn matches_enhanced_patterns(&self, query: &str, name: &str) -> bool {
        let query = query.to_lowercase();

        // 1. Standard substring match (already covered by search_by_field)

        // 2. Word-based matching - split query into words and find them
        let query_words: Vec<&str> = query.split_whitespace().collect();
        if query_words.len() > 1 {
            let all_words_found = query_words.iter().all(|word| name.contains(word));
            if all_words_found {
                return true;
            }
        }

        // 3. Handle common separators (hyphens, underscores, dots)
        let normalized_name = name.replace("-", "").replace("_", "").replace(".", "");

        let normalized_query = query.replace("-", "").replace("_", "").replace(".", "");

        // Check if normalized versions match
        if normalized_name.contains(&normalized_query) {
            return true;
        }

        // 4. Acronym matching (first letters of words)
        if query.len() >= 2 {
            let words: Vec<&str> = name.split(&['-', '_', ' '][..]).collect();
            if words.len() > 1 {
                let acronym: String = words
                    .iter()
                    .filter_map(|word| word.chars().next())
                    .collect();
                if acronym.to_lowercase().contains(&query) {
                    return true;
                }
            }
        }

        // 5. Partial acronym matching
        if query.len() >= 2 {
            let name_chars: String = name.chars().filter(|c| c.is_alphanumeric()).collect();
            if name_chars.to_lowercase().starts_with(&query) {
                return true;
            }
        }

        false
    }

    fn search_in_tags(&self, query: &str, limit: usize) -> Result<Vec<Connection>> {
        let sql =
            "SELECT id, name, host, user, port, bastion, bastion_user, use_kerberos, key_path, created_at, last_used, tags
             FROM connections
             WHERE tags LIKE ? COLLATE NOCASE
             ORDER BY last_used DESC NULLS LAST, name ASC
             LIMIT ?";

        let mut stmt = self.conn.prepare(sql)?;
        let like_pattern = format!("%\"{}\"%", query);
        let mut rows = stmt.query(params![like_pattern, limit])?;

        let mut connections = Vec::new();
        while let Some(row) = rows.next()? {
            connections.push(self.row_to_connection(row)?);
        }

        Ok(connections)
    }

    fn deduplicate_and_rank(&self, connections: &mut Vec<Connection>, query: &str, mode: &str) {
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

        // Sort by relevance score based on mode
        connections.sort_by(|a, b| {
            let score_a = if mode == "bayesian" {
                self.calculate_bayesian_score(a, query)
            } else {
                self.calculate_relevance_score(a, query)
            };
            let score_b = if mode == "bayesian" {
                self.calculate_bayesian_score(b, query)
            } else {
                self.calculate_relevance_score(b, query)
            };
            score_b
                .partial_cmp(&score_a)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    }

    /// Bayesian-inspired scoring that combines:
    /// - Prior probability (frequency of use)
    /// - Likelihood (match quality)
    /// - Recency (temporal decay)
    fn calculate_bayesian_score(&self, connection: &Connection, query: &str) -> f64 {
        // Get connection usage statistics
        let (frequency, total_connections) = self.get_connection_frequency(&connection.id);

        // 1. Prior probability: P(connection) based on historical usage
        let prior = if total_connections > 0 {
            (frequency as f64 + 1.0) / (total_connections as f64 + 10.0) // Laplace smoothing
        } else {
            0.1 // Default prior for new connections
        };

        // 2. Likelihood: P(query | connection) - how well does query match?
        let likelihood = self.calculate_match_likelihood(connection, query);

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

    fn calculate_match_likelihood(&self, connection: &Connection, query: &str) -> f64 {
        let query_lower = query.to_lowercase();
        let name_lower = connection.name.to_lowercase();

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

    fn calculate_relevance_score(&self, connection: &Connection, query: &str) -> f64 {
        let query_lower = query.to_lowercase();
        let name_lower = connection.name.to_lowercase();
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
        if self.matches_enhanced_patterns(query, &name_lower) {
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
