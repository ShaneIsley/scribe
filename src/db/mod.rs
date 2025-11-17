pub mod backend;
pub mod sqlite;

// Re-export Database for backward compatibility (transitional)
pub use sqlite::Database;

#[cfg(test)]
mod tests {
    use super::backend::tests::*;
    use super::backend::DatabaseBackend;
    use super::sqlite::SqliteBackend;
    use anyhow::Result;
    use std::path::Path;

    fn create_sqlite_backend(path: &Path) -> Result<Box<dyn DatabaseBackend>> {
        Ok(Box::new(SqliteBackend::open(path)?))
    }

    #[test]
    fn test_sqlite_backend_lifecycle() {
        test_backend_lifecycle(create_sqlite_backend);
    }

    #[test]
    fn test_sqlite_backend_update_item() {
        test_backend_update_item(create_sqlite_backend);
    }

    #[test]
    fn test_sqlite_backend_get_data_columns() {
        test_backend_get_data_columns(create_sqlite_backend);
    }
}
