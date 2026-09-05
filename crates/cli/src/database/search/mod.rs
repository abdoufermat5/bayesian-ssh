//! Bayesian and fuzzy search over the connections table.
//!
//! Split by concern:
//! - `query`   — entry points and SQL/pattern querying
//! - `scoring` — deduplication, relevance and Bayesian ranking

mod query;
mod scoring;

pub use query::SearchField;

#[cfg(test)]
mod tests {
    use crate::config::AppConfig;
    use crate::database::{Database, SearchField};
    use crate::models::Connection;
    use tempfile::tempdir;

    #[test]
    fn test_tag_prefix_search() {
        let dir = tempdir().unwrap();
        let config = AppConfig {
            database_path: dir.path().join("test.db"),
            ..Default::default()
        };
        let db = Database::new(&config).unwrap();

        let mut conn1 = Connection::new(
            "web-prod".into(),
            "web1.example.com".into(),
            "root".into(),
            22,
            None,
            None,
            false,
            None,
        );
        conn1.add_tag("production".into());
        db.add_connection(&conn1).unwrap();

        let mut conn2 = Connection::new(
            "web-staging".into(),
            "web2.example.com".into(),
            "root".into(),
            22,
            None,
            None,
            false,
            None,
        );
        conn2.add_tag("staging".into());
        db.add_connection(&conn2).unwrap();

        let res_tag = db
            .search_connections("tag:production", 10, "bayesian")
            .unwrap();
        assert_eq!(res_tag.len(), 1);
        assert_eq!(res_tag[0].name, "web-prod");

        let res_at = db.search_connections("@staging", 10, "bayesian").unwrap();
        assert_eq!(res_at.len(), 1);
        assert_eq!(res_at[0].name, "web-staging");
    }

    #[test]
    fn test_search_by_field_is_type_safe() {
        // The new `search_by_field` takes a `SearchField` enum, so
        // arbitrary strings (even SQL-injection attempts) cannot reach
        // the SQL builder.
        let dir = tempdir().unwrap();
        let config = AppConfig {
            database_path: dir.path().join("test.db"),
            ..Default::default()
        };
        let db = Database::new(&config).unwrap();

        // These all type-check and never reach the DB as raw text.
        let _ = db.search_by_field("query", SearchField::Name, 10);
        let _ = db.search_by_field("query", SearchField::Host, 10);
        let _ = db.search_by_field("query", SearchField::Tags, 10);
    }

    #[test]
    fn test_search_by_field_accepts_valid_fields() {
        let dir = tempdir().unwrap();
        let config = AppConfig {
            database_path: dir.path().join("test.db"),
            ..Default::default()
        };
        let db = Database::new(&config).unwrap();

        let conn1 = Connection::new(
            "web-prod".into(),
            "web1.example.com".into(),
            "root".into(),
            22,
            None,
            None,
            false,
            None,
        );
        db.add_connection(&conn1).unwrap();

        for field in [SearchField::Name, SearchField::Host] {
            let result = db.search_by_field("web", field, 10);
            assert!(
                result.is_ok(),
                "field '{field:?}' should be allowed: {:?}",
                result.err()
            );
            assert!(!result.as_ref().unwrap().is_empty(), "should find 'web'");
        }
    }
}
