# Scribe Product Requirements Document

## Overview
Scribe is a high-performance Git history database extractor. This document outlines the feature roadmap across multiple versions.

## Version History

### v1.0 (Released)
- SQLite database output
- JSON and CSV file format support
- Incremental updates with last-commit tracking
- Parallel blob parsing with rayon
- Full version history tracking
- Composite primary key support

---

## v1.1 Features

### 1. DuckDB Output Support

**Motivation:** DuckDB provides superior analytical query performance with columnar storage, making it ideal for large-scale data analysis and time-series queries on version history.

**Requirements:**
- Add `--backend` CLI flag accepting `sqlite` (default) or `duckdb`
- Create database backend abstraction layer
- Implement DuckDB backend with same schema as SQLite
- Maintain backward compatibility with existing SQLite databases

**Schema (same for both backends):**
- `commits` table: commit hash and timestamp
- `items` table: current state of all items
- `item_versions` table: complete version history

**Example Usage:**
```bash
# Use DuckDB instead of SQLite
scribe history.duckdb data/users.json -p id --backend duckdb -v

# Query with DuckDB's analytical features
duckdb history.duckdb -c "
  SELECT
    DATE_TRUNC('month', c.commit_at) as month,
    COUNT(DISTINCT iv._item_pk) as changed_items
  FROM item_versions iv
  JOIN commits c ON iv._commit_hash = c.hash
  GROUP BY month
  ORDER BY month
"
```

**Technical Approach:**
- Create `DatabaseBackend` trait in `src/db/backend.rs`
- Move existing code to `src/db/sqlite.rs` as `SQLiteBackend`
- Create new `src/db/duckdb.rs` with `DuckDBBackend`
- Use trait objects in `processor.rs`

---

### 2. Diff-Only Storage Mode

**Motivation:** For items with many columns where only a few fields change frequently, storing full snapshots wastes storage. Diff-only mode stores only changed fields, reducing database size by 50-90% for typical use cases.

**Requirements:**
- Add `--diff-only` CLI flag (boolean)
- Modify `item_versions` table schema to include:
  - `_diff_type` TEXT: 'INSERT', 'UPDATE', or 'DELETE'
  - `_changed_fields` TEXT: JSON array of field names that changed
  - Data columns: NULL for unchanged fields, values only for changed fields
- Compute field-level diffs between previous and current item states
- Store only the delta in `item_versions`

**Example Schema:**
```sql
-- Traditional mode (v1.0)
item_versions: _version_id, _item_pk, _commit_hash, id, name, age, email, phone, address, ...
Row: 1, 'abc', 'def123', '1', 'Alice', '31', 'alice@example.com', '555-1234', '123 Main St', ...

-- Diff-only mode (v1.1)
item_versions: _version_id, _item_pk, _commit_hash, _diff_type, _changed_fields, id, name, age, email, phone, address, ...
Row: 2, 'abc', 'ghi456', 'UPDATE', '["age","email"]', NULL, NULL, '32', 'alice@newmail.com', NULL, NULL, ...
```

**Example Usage:**
```bash
# Enable diff-only mode for space savings
scribe history.db data/users.json -p id --diff-only -v

# Reconstruct full state by applying diffs chronologically
SELECT * FROM reconstruct_item('abc123')  -- hypothetical function
```

**Technical Approach:**
- Create `src/diff.rs` module with `compute_diff()` function
- Update `Database::init_schema()` to conditionally add diff columns
- Update `processor.rs` to track previous state and compute diffs
- Store diff metadata in `_diff_type` and `_changed_fields`

---

### 3. JMESPath Transformations

**Motivation:** Source data often contains nested structures, unwanted fields, or needs reshaping before tracking. JMESPath allows powerful data transformations without modifying source files.

**Requirements:**
- Add `--transform <EXPRESSION>` CLI flag accepting JMESPath expression
- Apply transformation after parsing but before primary key computation
- Support both JSON and CSV inputs (CSV converted to JSON internally first)
- Fail early with clear error if transformation is invalid

**Example Use Cases:**

1. **Flatten nested objects:**
```bash
# Input: [{"id": 1, "author": {"name": "Alice", "email": "alice@example.com"}}]
scribe history.db data.json -p id --transform "[].{id: id, author_name: author.name}" -v

# Output schema: id, author_name
```

2. **Filter fields:**
```bash
# Only track specific fields from large records
scribe history.db data.json -p id --transform "[].{id: id, status: status, updated: updated_at}" -v
```

3. **Extract array elements:**
```bash
# Input: [{"id": 1, "tags": ["python", "rust", "go"]}]
scribe history.db data.json -p id --transform "[].{id: id, primary_tag: tags[0]}" -v
```

**Example Usage:**
```bash
# Transform GitHub API response to track only relevant fields
scribe repos.db github_repos.json -p id \
  --transform "[].{id: id, name: name, stars: stargazers_count, owner: owner.login}" -v
```

**Technical Approach:**
- Add `jmespath` crate dependency
- Create `src/transform.rs` module with `apply_transform()` function
- Update `parser.rs` to accept optional transform parameter
- Apply transform after `parse_file()` but before returning items

**Error Handling:**
- Invalid JMESPath expression → fail fast with syntax error
- Transform returns non-array → fail with clear message
- Transform returns non-objects → fail with clear message

---

## v1.2 Features (Future)

### 1. PostgreSQL Support
- Add PostgreSQL as third backend option
- Better for multi-user scenarios and remote access
- Native JSON/JSONB support for flexible schemas

### 2. Configuration Files
- Support `.scriberc` or `scribe.toml` for persistent settings
- Avoid repeating CLI flags for regular workflows
- Per-repository configuration

### 3. Basic Normalization
- Auto-detect related entities (foreign keys)
- Create separate tables for 1:N relationships
- Reduce redundancy in highly normalized data

---

## v2.0 Features (Future)

### 1. Dolt Integration
- Export to Dolt format for Git-like version control of database
- Bidirectional sync between Scribe and Dolt
- Leverage Dolt's branching/merging for data

### 2. Multiple File Tracking
- Track multiple files in single database
- Maintain relationships between files
- Cross-file queries and joins

### 3. Schema Evolution
- Handle column additions/removals gracefully
- Migration system for schema changes
- Automatic column type detection and conversion

---

## Testing Requirements

### v1.1 Test Coverage
- **JMESPath:**
  - Valid transformations (flatten, filter, extract)
  - Invalid expressions (syntax errors)
  - Edge cases (empty arrays, null values)

- **DuckDB:**
  - All CRUD operations
  - Schema creation
  - Query compatibility with SQLite
  - Large dataset performance tests

- **Diff-only:**
  - Field-level diff computation
  - INSERT/UPDATE/DELETE detection
  - Null vs changed field differentiation
  - Storage size reduction validation

### Integration Tests
- End-to-end workflow with all flags combined
- Incremental updates with each mode
- Performance benchmarks vs v1.0

---

## Performance Goals

### v1.1 Targets
- **DuckDB:** 2-5x faster analytical queries vs SQLite
- **Diff-only:** 50-90% storage reduction for typical workloads
- **JMESPath:** <10% overhead vs no transformation

### Benchmarking
- Test with 10k commits, 100k items
- Measure parse time, storage size, query performance
- Compare against git-history baseline
