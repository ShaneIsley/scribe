use anyhow::Result;
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use std::path::Path;

/// Database backend abstraction trait
/// Allows switching between SQLite and DuckDB while maintaining the same interface
pub trait DatabaseBackend: Send {
    /// Initialize the database schema with the given columns
    fn init_schema(&self, columns: &[String]) -> Result<()>;

    /// Get the last processed commit hash from metadata
    fn get_last_processed_commit(&self) -> Result<Option<String>>;

    /// Set the last processed commit hash in metadata
    fn set_last_processed_commit(&self, hash: &str) -> Result<()>;

    /// Get all data column names (excluding special columns starting with _)
    fn get_data_columns(&self) -> Result<Vec<String>>;

    /// Begin a transaction and return a transaction handle
    fn begin_transaction(&mut self) -> Result<Box<dyn Transaction + '_>>;

    /// Load the current state of items from the database
    /// Returns a HashMap of item_pk -> content_hash
    fn load_current_state(&self) -> Result<HashMap<String, String>>;
}

/// Transaction abstraction trait
/// Allows database operations within a transaction
pub trait Transaction {
    /// Insert a commit record
    fn insert_commit(&self, hash: &str, commit_at: &DateTime<Utc>) -> Result<()>;

    /// Insert a new item
    fn insert_item(
        &self,
        item_pk: &str,
        commit_hash: &str,
        columns: &[String],
        item: &BTreeMap<String, Value>,
    ) -> Result<()>;

    /// Update an existing item
    fn update_item(
        &self,
        item_pk: &str,
        commit_hash: &str,
        columns: &[String],
        item: &BTreeMap<String, Value>,
    ) -> Result<()>;

    /// Insert a new item version
    fn insert_item_version(
        &self,
        item_pk: &str,
        commit_hash: &str,
        columns: &[String],
        item: &BTreeMap<String, Value>,
    ) -> Result<()>;

    /// Commit the transaction
    fn commit(self: Box<Self>) -> Result<()>;
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::NamedTempFile;

    // Test helper to create a sample item
    fn make_test_item() -> BTreeMap<String, Value> {
        let mut item = BTreeMap::new();
        item.insert("id".to_string(), json!("1"));
        item.insert("name".to_string(), json!("Alice"));
        item.insert("age".to_string(), json!(30));
        item
    }

    // Generic tests that work for any backend
    pub fn test_backend_lifecycle<F>(create_backend: F)
    where
        F: Fn(&Path) -> Result<Box<dyn DatabaseBackend>>,
    {
        let temp_file = NamedTempFile::new().unwrap();
        let db_path = temp_file.path();

        // Create backend
        let mut backend = create_backend(db_path).unwrap();

        // Initialize schema
        let columns = vec!["id".to_string(), "name".to_string(), "age".to_string()];
        backend.init_schema(&columns).unwrap();

        // Check no last commit initially
        assert_eq!(backend.get_last_processed_commit().unwrap(), None);

        // Begin transaction
        let tx = backend.begin_transaction().unwrap();

        // Insert commit
        let now = Utc::now();
        tx.insert_commit("abc123", &now).unwrap();

        // Insert item
        let item = make_test_item();
        tx.insert_item("item1", "abc123", &columns, &item).unwrap();

        // Insert item version
        tx.insert_item_version("item1", "abc123", &columns, &item)
            .unwrap();

        // Commit transaction
        tx.commit().unwrap();

        // Set last processed commit
        backend.set_last_processed_commit("abc123").unwrap();

        // Verify last commit
        assert_eq!(
            backend.get_last_processed_commit().unwrap(),
            Some("abc123".to_string())
        );

        // Load current state
        let state = backend.load_current_state().unwrap();
        assert!(state.contains_key("item1"));
    }

    pub fn test_backend_update_item<F>(create_backend: F)
    where
        F: Fn(&Path) -> Result<Box<dyn DatabaseBackend>>,
    {
        let temp_file = NamedTempFile::new().unwrap();
        let db_path = temp_file.path();

        let mut backend = create_backend(db_path).unwrap();
        let columns = vec!["id".to_string(), "name".to_string(), "age".to_string()];
        backend.init_schema(&columns).unwrap();

        // Insert initial item
        let tx = backend.begin_transaction().unwrap();
        let now = Utc::now();
        tx.insert_commit("commit1", &now).unwrap();

        let item1 = make_test_item();
        tx.insert_item("item1", "commit1", &columns, &item1).unwrap();
        tx.insert_item_version("item1", "commit1", &columns, &item1)
            .unwrap();
        tx.commit().unwrap();

        // Update item
        let tx = backend.begin_transaction().unwrap();
        tx.insert_commit("commit2", &now).unwrap();

        let mut item2 = make_test_item();
        item2.insert("age".to_string(), json!(31));

        tx.update_item("item1", "commit2", &columns, &item2).unwrap();
        tx.insert_item_version("item1", "commit2", &columns, &item2)
            .unwrap();
        tx.commit().unwrap();

        // Verify state updated
        let state = backend.load_current_state().unwrap();
        assert_eq!(state.len(), 1);
        assert!(state.contains_key("item1"));
    }

    pub fn test_backend_get_data_columns<F>(create_backend: F)
    where
        F: Fn(&Path) -> Result<Box<dyn DatabaseBackend>>,
    {
        let temp_file = NamedTempFile::new().unwrap();
        let db_path = temp_file.path();

        let backend = create_backend(db_path).unwrap();
        let columns = vec!["id".to_string(), "name".to_string(), "age".to_string()];
        backend.init_schema(&columns).unwrap();

        let data_cols = backend.get_data_columns().unwrap();
        assert_eq!(data_cols.len(), 3);
        assert!(data_cols.contains(&"id".to_string()));
        assert!(data_cols.contains(&"name".to_string()));
        assert!(data_cols.contains(&"age".to_string()));

        // Should not contain special columns
        assert!(!data_cols.iter().any(|c| c.starts_with('_')));
    }
}
