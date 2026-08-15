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
        let mut normalized_query = query.trim().to_lowercase();

        let is_tag_query = if let Some(stripped) = normalized_query.strip_prefix("tag:") {
            normalized_query = stripped.to_string();
            true
        } else if let Some(stripped) = normalized_query.strip_prefix('@') {
            normalized_query = stripped.to_string();
            true
        } else {
            false
        };

        if is_tag_query {
            if let Ok(mut tag_matches) = self.search_in_tags(&normalized_query, limit) {
                all_matches.append(&mut tag_matches);
            }
        } else {
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
        }

        // Remove duplicates and sort by relevance
        self.deduplicate_and_rank(&mut all_matches, &normalized_query, mode);

        // Limit results
        all_matches.truncate(limit);

        Ok(all_matches)
    }

    pub(super) fn search_by_field(&self, query: &str, field: &str, limit: usize) -> Result<Vec<Connection>> {
        const ALLOWED_FIELDS: [&str; 3] = ["name", "host", "tags"];
        if !ALLOWED_FIELDS.contains(&field) {
            return Err(anyhow::anyhow!(
                "Invalid search field '{field}': must be one of name, host, tags"
            ));
        }

        let sql = format!(
            "SELECT id, name, host, user, port, bastion, bastion_user, use_kerberos, key_path, created_at, last_used, tags
             FROM connections
             WHERE {field} LIKE ? COLLATE NOCASE
             ORDER BY last_used DESC NULLS LAST, name ASC
             LIMIT ?"
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

    pub(super) fn matches_enhanced_patterns(&self, query: &str, name: &str) -> bool {
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
}
