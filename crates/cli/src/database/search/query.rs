use crate::database::search::RankContext;
use crate::database::{Database, escape_like};
use crate::models::Connection;
use anyhow::Result;
use rusqlite::params;

/// Precomputed lowercase / normalized representations of a search query so
/// candidate matching avoids repeating heap allocations per connection.
pub(super) struct PreparedQuery {
    pub(super) lower: String,
    pub(super) words: Vec<String>,
    pub(super) normalized: String,
    pub(super) prefix_digits: Option<(String, String)>,
}

impl PreparedQuery {
    pub(super) fn new(query: &str) -> Self {
        let lower = query.to_lowercase();
        let words: Vec<String> = lower.split_whitespace().map(String::from).collect();
        let normalized: String = lower
            .chars()
            .filter(|&c| !matches!(c, '-' | '_' | '.'))
            .collect();
        let digits_start = lower.trim_end_matches(|c: char| c.is_ascii_digit()).len();
        let (word, digits) = lower.split_at(digits_start);
        let prefix_digits = if !digits.is_empty()
            && word.chars().count() >= 2
            && word.chars().all(|c| c.is_alphabetic())
        {
            Some((word.to_string(), digits.to_string()))
        } else {
            None
        };
        Self {
            lower,
            words,
            normalized,
            prefix_digits,
        }
    }
}

/// The set of fields the unified search supports. Using an enum (instead of
/// a `&str`) makes the column-name interpolation type-safe and prevents
/// accidental SQL injection if a caller passes an unvalidated value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchField {
    Name,
    Host,
}

impl SearchField {
    fn column(self) -> &'static str {
        match self {
            SearchField::Name => "name",
            SearchField::Host => "host",
        }
    }
}

impl Database {
    // ──────────────────────────────────────────────────────────────────────────
    // Public search entry points
    // ──────────────────────────────────────────────────────────────────────────

    pub fn fuzzy_search_connections(&self, query: &str, limit: usize) -> Result<Vec<Connection>> {
        self.search_connections(query, limit, "fuzzy")
    }

    pub fn bayesian_search_connections(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<Connection>> {
        self.search_connections(query, limit, "bayesian")
    }

    pub fn search_connections(
        &self,
        query: &str,
        limit: usize,
        mode: &str,
    ) -> Result<Vec<Connection>> {
        self.search_connections_ctx(query, limit, mode, None)
    }

    pub fn search_connections_ctx(
        &self,
        query: &str,
        limit: usize,
        mode: &str,
        ctx: Option<&RankContext>,
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
            if let Some(tag) = Connection::normalize_tag(&normalized_query)
                && let Ok(mut tag_matches) = self.search_in_tags(&tag, limit)
            {
                all_matches.append(&mut tag_matches);
            }
        } else {
            if let Ok(mut name_matches) =
                self.search_by_field(&normalized_query, SearchField::Name, limit)
            {
                all_matches.append(&mut name_matches);
            }
            if let Ok(mut fuzzy_matches) = self.enhanced_fuzzy_search(&normalized_query, limit) {
                all_matches.append(&mut fuzzy_matches);
            }
            if let Ok(mut host_matches) =
                self.search_by_field(&normalized_query, SearchField::Host, limit)
            {
                all_matches.append(&mut host_matches);
            }
            // Exact match against the normalized `connection_tags` table.
            if let Ok(mut tag_matches) = self.search_in_tags(&normalized_query, limit) {
                all_matches.append(&mut tag_matches);
            }
        }

        // The per-source queries return rows without tags. Load them once
        // for every hit so ranking can match on tags and callers (e.g.
        // `edit`, which writes the connection back) never see an empty tag
        // set that would wipe the stored tags.
        let mut ids: Vec<String> = all_matches.iter().map(|c| c.id.to_string()).collect();
        ids.sort_unstable();
        ids.dedup();
        let tag_map = self.tags_for_connections(&ids)?;
        for conn in &mut all_matches {
            if let Some(tags) = tag_map.get(&conn.id.to_string()) {
                conn.tags = tags.clone();
            }
        }

        self.deduplicate_and_rank(&mut all_matches, &normalized_query, mode, ctx);
        all_matches.truncate(limit);
        Ok(all_matches)
    }

    // ──────────────────────────────────────────────────────────────────────────
    // Field-specific search
    // ──────────────────────────────────────────────────────────────────────────

    /// Type-safe column search. `field` is a `SearchField` enum so the column
    /// name can never be a user-controlled string.
    pub(super) fn search_by_field(
        &self,
        query: &str,
        field: SearchField,
        limit: usize,
    ) -> Result<Vec<Connection>> {
        let column = field.column();
        let sql = format!(
            "SELECT id, name, host, user, port, bastion, bastion_user, use_kerberos, key_path, created_at, last_used, source_id, external_id, stale_since, forward_agent, pending_change
             FROM connections
             WHERE {column} LIKE ? ESCAPE '\\' COLLATE NOCASE
             ORDER BY last_used DESC NULLS LAST, name ASC
             LIMIT ?"
        );

        // Escape user-supplied LIKE wildcards so `%` and `_` in a
        // connection name are treated literally.
        let escaped = escape_like(query);
        let like_pattern = format!("%{}%", escaped);

        let mut stmt = self.conn.prepare(&sql)?;
        let mut rows = stmt.query(params![like_pattern, limit as i64])?;

        let mut connections = Vec::new();
        while let Some(row) = rows.next()? {
            connections.push(self.row_to_connection(row)?);
        }
        Ok(connections)
    }

    fn enhanced_fuzzy_search(&self, query: &str, limit: usize) -> Result<Vec<Connection>> {
        let prepared = PreparedQuery::new(query);
        let sql = "SELECT id, name, host, user, port, bastion, bastion_user, use_kerberos, key_path, created_at, last_used, source_id, external_id, stale_since, forward_agent, pending_change
                   FROM connections
                   ORDER BY last_used DESC NULLS LAST, name ASC";
        let mut stmt = self.conn.prepare(sql)?;
        let mut rows = stmt.query([])?;
        let mut connections = Vec::new();
        while let Some(row) = rows.next()? {
            let connection = self.row_to_connection(row)?;
            let name_lower = connection.name.to_lowercase();
            if self.matches_prepared_patterns(&prepared, &name_lower) {
                connections.push(connection);
                if connections.len() >= limit {
                    break;
                }
            }
        }
        Ok(connections)
    }

    /// Exact-match search against the normalized `connection_tags` table.
    /// Replaces the old JSON-LIKE query that produced false positives.
    pub(super) fn search_in_tags(&self, query: &str, limit: usize) -> Result<Vec<Connection>> {
        let sql = "SELECT c.id, c.name, c.host, c.user, c.port, c.bastion, c.bastion_user,
                          c.use_kerberos, c.key_path, c.created_at, c.last_used,
                          c.source_id, c.external_id, c.stale_since, c.forward_agent, c.pending_change
                   FROM connections c
                   JOIN connection_tags ct ON c.id = ct.connection_id
                   WHERE ct.tag = ?
                   ORDER BY c.last_used DESC NULLS LAST, c.name ASC
                   LIMIT ?";
        let mut stmt = self.conn.prepare(sql)?;
        let mut rows = stmt.query(params![query, limit as i64])?;
        let mut connections = Vec::new();
        while let Some(row) = rows.next()? {
            connections.push(self.row_to_connection(row)?);
        }
        Ok(connections)
    }

    pub(super) fn matches_prepared_patterns(
        &self,
        query: &PreparedQuery,
        name_lower: &str,
    ) -> bool {
        if query.words.len() > 1
            && query
                .words
                .iter()
                .all(|word| name_lower.contains(word.as_str()))
        {
            return true;
        }

        if name_lower.contains(['-', '_', '.']) {
            let normalized_name: String = name_lower
                .chars()
                .filter(|&c| !matches!(c, '-' | '_' | '.'))
                .collect();
            if normalized_name.contains(query.normalized.as_str()) {
                return true;
            }
        } else if name_lower.contains(query.normalized.as_str()) {
            return true;
        }

        if query.lower.len() >= 2 {
            let mut parts = name_lower.split(['-', '_', ' ']);
            if let (Some(w0), Some(w1)) = (parts.next(), parts.next()) {
                let mut acronym = String::new();
                if let Some(c) = w0.chars().next() {
                    acronym.push(c);
                }
                if let Some(c) = w1.chars().next() {
                    acronym.push(c);
                }
                for w in parts {
                    if let Some(c) = w.chars().next() {
                        acronym.push(c);
                    }
                }
                if acronym.contains(query.lower.as_str()) {
                    return true;
                }
            }

            let mut name_alnum = name_lower.chars().filter(|c| c.is_alphanumeric());
            if query.lower.chars().all(|qc| name_alnum.next() == Some(qc)) {
                return true;
            }
        }

        // `web1` -> `web-prod-1`: letters that start one name segment, followed
        // by the digits of a later segment.
        if let Some((ref word, ref digits)) = query.prefix_digits {
            let mut saw_word = false;
            for seg in name_lower
                .split(['-', '_', '.', ' '])
                .filter(|s| !s.is_empty())
            {
                if saw_word && seg == digits.as_str() {
                    return true;
                }
                if seg.starts_with(word.as_str()) {
                    saw_word = true;
                }
            }
        }

        false
    }
}
