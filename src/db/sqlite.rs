use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use std::collections::{BTreeMap, HashMap};
use std::path::Path;

/// Database manager for Scribe
pub struct Database {
    conn: Connection,
}

impl Database {
    /// Open or create a database at the given path
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)
            .with_context(|| format!("Failed to open database at {}", path.display()))?;

        Ok(Self { conn })
    }

    /// Initialize the database schema
    pub fn init_schema(&self, columns: &[String]) -> Result<()> {
        // Create metadata table
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS _scribe_meta (
                key TEXT PRIMARY KEY,
                value TEXT
            )",
            [],
        )?;

        // Create commits table
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS commits (
                hash TEXT PRIMARY KEY,
                commit_at DATETIME NOT NULL
            )",
            [],
        )?;

        // Build dynamic column definitions for items table
        let data_columns = columns
            .iter()
            .map(|col| format!("\"{}\" TEXT", col))
            .collect::<Vec<_>>()
            .join(", ");

        // Create items table with dynamic columns
        let items_sql = format!(
            "CREATE TABLE IF NOT EXISTS items (
                _item_pk TEXT PRIMARY KEY,
                _last_commit_hash TEXT NOT NULL,
                {},
                FOREIGN KEY (_last_commit_hash) REFERENCES commits(hash)
            )",
            data_columns
        );
        self.conn.execute(&items_sql, [])?;

        // Create item_versions table with dynamic columns
        let item_versions_sql = format!(
            "CREATE TABLE IF NOT EXISTS item_versions (
                _version_id INTEGER PRIMARY KEY AUTOINCREMENT,
                _item_pk TEXT NOT NULL,
                _commit_hash TEXT NOT NULL,
                {},
                FOREIGN KEY (_item_pk) REFERENCES items(_item_pk),
                FOREIGN KEY (_commit_hash) REFERENCES commits(hash)
            )",
            data_columns
        );
        self.conn.execute(&item_versions_sql, [])?;

        // Create indexes for better query performance
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_items_last_commit
             ON items(_last_commit_hash)",
            [],
        )?;

        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_item_versions_pk
             ON item_versions(_item_pk)",
            [],
        )?;

        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_item_versions_commit
             ON item_versions(_commit_hash)",
            [],
        )?;

        Ok(())
    }

    /// Get the last processed commit hash from metadata
    /// Returns None if the table doesn't exist yet or no commit has been processed
    pub fn get_last_processed_commit(&self) -> Result<Option<String>> {
        // First, check if the metadata table exists
        let table_exists: bool = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='_scribe_meta'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .map(|count| count > 0)
            .unwrap_or(false);

        if !table_exists {
            return Ok(None);
        }

        let result: Option<String> = self
            .conn
            .query_row(
                "SELECT value FROM _scribe_meta WHERE key = 'last_processed_commit'",
                [],
                |row| row.get(0),
            )
            .optional()?;

        Ok(result)
    }

    /// Set the last processed commit hash in metadata
    pub fn set_last_processed_commit(&self, hash: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO _scribe_meta (key, value) VALUES ('last_processed_commit', ?1)",
            params![hash],
        )?;
        Ok(())
    }

    /// Get all column names from the items table (excluding special columns)
    pub fn get_data_columns(&self) -> Result<Vec<String>> {
        let stmt = self
            .conn
            .prepare("SELECT * FROM items LIMIT 0")?;

        let columns: Vec<String> = stmt
            .column_names()
            .into_iter()
            .map(|s| s.to_string())
            .filter(|col| !col.starts_with('_'))
            .collect();

        Ok(columns)
    }

    /// Begin a transaction
    pub fn begin_transaction(&mut self) -> Result<Transaction<'_>> {
        Ok(self.conn.transaction()?)
    }

    /// Insert a commit record
    pub fn insert_commit(
        tx: &Transaction,
        hash: &str,
        commit_at: &DateTime<Utc>,
    ) -> Result<()> {
        tx.execute(
            "INSERT OR IGNORE INTO commits (hash, commit_at) VALUES (?1, ?2)",
            params![hash, commit_at],
        )?;
        Ok(())
    }

    /// Insert a new item
    pub fn insert_item(
        tx: &Transaction,
        item_pk: &str,
        commit_hash: &str,
        columns: &[String],
        item: &BTreeMap<String, serde_json::Value>,
    ) -> Result<()> {
        let placeholders = (0..columns.len() + 2)
            .map(|i| format!("?{}", i + 1))
            .collect::<Vec<_>>()
            .join(", ");

        let column_names = columns
            .iter()
            .map(|col| format!("\"{}\"", col))
            .collect::<Vec<_>>()
            .join(", ");

        let sql = format!(
            "INSERT INTO items (_item_pk, _last_commit_hash, {}) VALUES ({})",
            column_names, placeholders
        );

        let mut params: Vec<&dyn rusqlite::ToSql> = vec![&item_pk, &commit_hash];
        let values: Vec<String> = columns
            .iter()
            .map(|col| {
                item.get(col)
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "null".to_string())
            })
            .collect();

        for val in &values {
            params.push(val);
        }

        tx.execute(&sql, params.as_slice())?;
        Ok(())
    }

    /// Update an existing item
    pub fn update_item(
        tx: &Transaction,
        item_pk: &str,
        commit_hash: &str,
        columns: &[String],
        item: &BTreeMap<String, serde_json::Value>,
    ) -> Result<()> {
        let set_clause = columns
            .iter()
            .enumerate()
            .map(|(i, col)| format!("\"{}\" = ?{}", col, i + 3))
            .collect::<Vec<_>>()
            .join(", ");

        let sql = format!(
            "UPDATE items SET _last_commit_hash = ?1, {} WHERE _item_pk = ?2",
            set_clause
        );

        let mut params: Vec<&dyn rusqlite::ToSql> = vec![&commit_hash, &item_pk];
        let values: Vec<String> = columns
            .iter()
            .map(|col| {
                item.get(col)
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "null".to_string())
            })
            .collect();

        for val in &values {
            params.push(val);
        }

        tx.execute(&sql, params.as_slice())?;
        Ok(())
    }

    /// Insert a new item version
    pub fn insert_item_version(
        tx: &Transaction,
        item_pk: &str,
        commit_hash: &str,
        columns: &[String],
        item: &BTreeMap<String, serde_json::Value>,
    ) -> Result<()> {
        let placeholders = (0..columns.len() + 2)
            .map(|i| format!("?{}", i + 1))
            .collect::<Vec<_>>()
            .join(", ");

        let column_names = columns
            .iter()
            .map(|col| format!("\"{}\"", col))
            .collect::<Vec<_>>()
            .join(", ");

        let sql = format!(
            "INSERT INTO item_versions (_item_pk, _commit_hash, {}) VALUES ({})",
            column_names, placeholders
        );

        let mut params: Vec<&dyn rusqlite::ToSql> = vec![&item_pk, &commit_hash];
        let values: Vec<String> = columns
            .iter()
            .map(|col| {
                item.get(col)
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "null".to_string())
            })
            .collect();

        for val in &values {
            params.push(val);
        }

        tx.execute(&sql, params.as_slice())?;
        Ok(())
    }

    /// Load the current state of items from the database
    /// Returns a HashMap of item_pk -> content_hash
    pub fn load_current_state(&self) -> Result<HashMap<String, String>> {
        let mut stmt = self.conn.prepare(
            "SELECT _item_pk, * FROM items"
        )?;

        let column_names: Vec<String> = stmt
            .column_names()
            .into_iter()
            .map(|s| s.to_string())
            .filter(|col| !col.starts_with('_'))
            .collect();

        let mut state = HashMap::new();

        let rows = stmt.query_map([], |row| {
            let item_pk: String = row.get(0)?;

            let mut item = BTreeMap::new();
            for (idx, col_name) in column_names.iter().enumerate() {
                // Skip the _item_pk column (0) and _last_commit_hash column (1)
                let value: Option<String> = row.get(idx + 2).ok();
                if let Some(val) = value {
                    if let Ok(json_val) = serde_json::from_str(&val) {
                        item.insert(col_name.clone(), json_val);
                    } else {
                        item.insert(col_name.clone(), serde_json::Value::String(val));
                    }
                }
            }

            let content_hash = crate::hash::compute_content_hash(&item);
            Ok((item_pk, content_hash))
        })?;

        for row in rows {
            let (pk, hash) = row?;
            state.insert(pk, hash);
        }

        Ok(state)
    }
}
