# Scribe

**A High-Performance Git History Database Extractor**

Scribe is a blazingly fast CLI tool written in Rust that converts Git commit history of tracked files (JSON/CSV) into a versioned SQLite database. It's designed to be 10x-100x faster than [simonw/git-history](https://github.com/simonw/git-history) by using native Git operations and efficient parallel processing.

## Features

- **High Performance**: Native Git operations using [gitoxide](https://github.com/Byron/gitoxide) - no subprocess overhead
- **Incremental Updates**: Idempotent operation - only processes new commits on subsequent runs
- **Multiple Formats**: Supports both JSON arrays and CSV files with headers
- **Full Version Tracking**: Maintains complete history of all item changes
- **Parallel Processing**: Uses [rayon](https://github.com/rayon-rs/rayon) to parallelize blob parsing
- **Simple CLI**: Easy-to-use command-line interface built with [clap](https://github.com/clap-rs/clap)

## Use Cases

- **Data Journalism**: Track changes to government websites or public datasets
- **DevOps/SRE**: Archive system state (cloud resources, configurations) committed to Git
- **Data Versioning**: Query historical changes to data files tracked in Git repositories
- **Audit Trails**: Maintain searchable history of data changes with full lineage

## Installation

### From Source

```bash
git clone https://github.com/yourusername/scribe.git
cd scribe
cargo build --release
```

The binary will be available at `target/release/scribe`.

## Usage

### Basic Syntax

```bash
scribe <DB_PATH> <FILE_IN_GIT> -p <PRIMARY_KEY> [OPTIONS]
```

### Arguments

- `DB_PATH`: Path to SQLite database file (will be created if it doesn't exist)
- `FILE_IN_GIT`: Path to the tracked file in the Git repository
- `-p, --primary-key <COLUMN>`: Column(s) that uniquely identify items (can be specified multiple times)

### Options

- `-r, --repo <PATH>`: Path to Git repository (default: current directory)
- `-v, --verbose`: Increase verbosity (-v, -vv, -vvv)

### Examples

#### Track a JSON file

```bash
scribe history.db data/users.json -p id -v
```

This processes `data/users.json` where each item has a unique `id` field.

#### Track a CSV file with composite key

```bash
scribe metrics.db logs/metrics.csv -p timestamp -p server_id -vv
```

This processes `logs/metrics.csv` using both `timestamp` and `server_id` as a composite primary key.

#### Incremental updates

Simply run the same command again - Scribe automatically detects the last processed commit:

```bash
scribe history.db data/users.json -p id -v
# Output: "No new commits to process. Database is up to date!"
```

## Database Schema

Scribe creates three tables in the SQLite database:

### `commits`

| Column      | Type     | Description                    |
|-------------|----------|--------------------------------|
| hash        | TEXT     | Full Git commit hash (PK)      |
| commit_at   | DATETIME | Author timestamp of commit     |

### `items`

| Column              | Type | Description                              |
|---------------------|------|------------------------------------------|
| _item_pk            | TEXT | Stable hash of primary key(s) (PK)       |
| _last_commit_hash   | TEXT | Last commit that modified this item (FK) |
| [data_columns]...   | TEXT | All columns from source data             |

### `item_versions`

| Column            | Type    | Description                           |
|-------------------|---------|---------------------------------------|
| _version_id       | INTEGER | Auto-incrementing version ID (PK)     |
| _item_pk          | TEXT    | Item this version belongs to (FK)     |
| _commit_hash      | TEXT    | Commit this version appeared in (FK)  |
| [data_columns]... | TEXT    | Full copy of all data columns         |

## How It Works

1. **Git History Walking**: Uses gitoxide to efficiently walk commit history for the specified file
2. **Parallel Parsing**: Processes file contents from all commits in parallel using rayon
3. **Change Detection**: Computes content hashes to detect actual changes (not just new commits)
4. **Incremental Updates**: Tracks last processed commit to avoid re-processing history
5. **Full Versioning**: Stores complete snapshots in `item_versions` for easy querying

## Performance

Scribe achieves 10x-100x performance improvements over git-history through:

- **Native Git Access**: Direct .git object database access (no subprocess overhead)
- **Parallel Processing**: CPU-bound parsing operations run in parallel
- **Efficient Hashing**: Fast change detection using SHA-256 content hashing
- **Optimized I/O**: Batch database operations with transactions

## Example Queries

Once you've built a database, you can query it with any SQLite client:

```sql
-- Get current state of all items
SELECT * FROM items;

-- Get full history of a specific item
SELECT * FROM item_versions
WHERE _item_pk = 'abc123...'
ORDER BY _version_id;

-- Find all items changed in a specific commit
SELECT * FROM item_versions
WHERE _commit_hash = 'def456...';

-- Track changes to a specific field over time
SELECT iv._commit_hash, c.commit_at, iv.name, iv.age
FROM item_versions iv
JOIN commits c ON iv._commit_hash = c.hash
WHERE iv._item_pk = 'abc123...'
ORDER BY c.commit_at;
```

## Roadmap

See the [Product Requirements Document](PRD.md) for details on planned features:

- **v1.1**: DuckDB output support, diff-only storage mode, JMESPath transformations
- **v1.2**: PostgreSQL support, configuration files, basic normalization
- **v2.0**: Dolt integration, multiple file tracking

## Requirements

- Rust 1.70 or later
- Git repository with tracked JSON/CSV files

## File Format Support

### JSON
- Must be an array of flat objects: `[{"id": 1, "name": "Alice"}, ...]`
- Each object represents one item
- All values are stored as text in SQLite

### CSV
- Must have a header row
- Each row represents one item
- Values are parsed as numbers/booleans when possible, otherwise stored as text

## Contributing

Contributions are welcome! Please feel free to submit issues or pull requests.

## License

MIT OR Apache-2.0

## Acknowledgments

Inspired by [simonw/git-history](https://github.com/simonw/git-history) by Simon Willison.
Built with amazing Rust libraries: [gitoxide](https://github.com/Byron/gitoxide), [rusqlite](https://github.com/rusqlite/rusqlite), [rayon](https://github.com/rayon-rs/rayon), and [clap](https://github.com/clap-rs/clap).
