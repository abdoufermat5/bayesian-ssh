use crate::database::Database;
use crate::models::Connection;
use anyhow::Result;
use rusqlite::params;

/// The set of fields the unified search supports. Using an enum (instead of
/// a `&str`) makes the column-name interpolation type-safe and prevents
/// accidental SQL injection if a caller passes an unvalidated value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchField {
    Name,
    Host,
    Tags,
}

impl SearchField {
    fn column(self) -> &'static str {
        match self {
            SearchField::Name => "name",
            SearchField::Host => "host",
            SearchField::Tags => "tags", // legacy JSON column, only used for fallback
        }
    }
}

impl Database {
    // ──────────────────────────────────────────────────────────────────────────
    // Public search entry points
    // ──────────────────────────────────────────────────────────────────────────

    #[allow(dead_code)]
    pub fn fuzzy_search_connections(&self, query: &str, limit: usize) -> Result<Vec<Connection>> {
        self.search_connections(query, limit, "fuzzy")
    }

    #[allow(dead_code)]
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
            if let Some(tag) = Connection::normalize_tag(&normalized_query) {
                if let Ok(mut tag_matches) = self.search_in_tags(&tag, limit) {
                    all_matches.append(&mut tag_matches);
                }
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
            // Tag search uses the normalized table; fall back to JSON LIKE
            // for legacy compatibility only when no normalised hits.
            if let Ok(mut tag_matches) = self.search_in_tags(&normalized_query, limit) {
                all_matches.append(&mut tag_matches);
            }
        }

        self.deduplicate_and_rank(&mut all_matches, &normalized_query, mode);
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
            "SELECT id, name, host, user, port, bastion, bastion_user, use_kerberos, key_path, created_at, last_used
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
        let mut rows = stmt.query(params![like_pattern, limit])?;

        let mut connections = Vec::new();
        let mut ids = Vec::new();
        while let Some(row) = rows.next()? {
            let conn = self.row_to_connection(row)?;
            ids.push(conn.id.to_string());
            connections.push(conn);
        }

        // Batch-fetch tags in a single query (avoids N+1).
        let tag_map = self.get_tags_for_connections(&ids)?;
        for conn in &mut connections {
            if let Some(tags) = tag_map.get(&conn.id.to_string()) {
                conn.tags = tags.clone();
            }
        }
        Ok(connections)
    }

    fn enhanced_fuzzy_search(&self, query: &str, limit: usize) -> Result<Vec<Connection>> {
        let sql = "SELECT id, name, host, user, port, bastion, bastion_user, use_kerberos, key_path, created_at, last_used
                   FROM connections
                   ORDER BY last_used DESC NULLS LAST, name ASC";
        let mut stmt = self.conn.prepare(sql)?;
        let mut rows = stmt.query([])?;
        let mut connections = Vec::new();
        while let Some(row) = rows.next()? {
            let connection = self.row_to_connection(row)?;
            let name_lower = connection.name.to_lowercase();
            if self.matches_enhanced_patterns(query, &name_lower) {
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
                          c.use_kerberos, c.key_path, c.created_at, c.last_used
                   FROM connections c
                   JOIN connection_tags ct ON c.id = ct.connection_id
                   WHERE ct.tag = ?
                   ORDER BY c.last_used DESC NULLS LAST, c.name ASC
                   LIMIT ?";
        let mut stmt = self.conn.prepare(sql)?;
        let mut rows = stmt.query(params![query, limit])?;
        let mut connections = Vec::new();
        while let Some(row) = rows.next()? {
            connections.push(self.row_to_connection(row)?);
        }
        Ok(connections)
    }

    pub(super) fn matches_enhanced_patterns(&self, query: &str, name: &str) -> bool {
        let query = query.to_lowercase();

        let query_words: Vec<&str> = query.split_whitespace().collect();
        if query_words.len() > 1 {
            let all_words_found = query_words.iter().all(|word| name.contains(word));
            if all_words_found {
                return true;
            }
        }

        let normalized_name = name.replace("-", "").replace("_", "").replace(".", "");
        let normalized_query = query.replace("-", "").replace("_", "").replace(".", "");

        if normalized_name.contains(&normalized_query) {
            return true;
        }

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

        if query.len() >= 2 {
            let name_chars: String = name.chars().filter(|c| c.is_alphanumeric()).collect();
            if name_chars.to_lowercase().starts_with(&query) {
                return true;
            }
        }

        false
    }
}

/// Escape SQL LIKE wildcards (`%`, `_`, `\`) in a user-supplied string.
/// Pair with `ESCAPE '\'` in the SQL query.
fn escape_like(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' | '%' | '_' => {
                out.push('\\');
                out.push(c);
            }
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::escape_like;

    #[test]
    fn escape_like_doubles_wildcards() {
        assert_eq!(escape_like("foo%bar"), "foo\\%bar");
        assert_eq!(escape_like("a_b"), "a\\_b");
        assert_eq!(escape_like("a\\b"), "a\\\\b");
        assert_eq!(escape_like("plain"), "plain");
    }
}
