# Scribe: A Code Walkthrough

*2026-03-16T22:37:36Z by Showboat 0.6.1*
<!-- showboat-id: 90ef4cfd-0ca4-4f0a-8d8b-c449752223a4 -->

Scribe is a CLI tool written in Rust that converts the Git commit history of a tracked JSON or CSV file into a versioned SQLite database. Each time a row changes, Scribe records the new values in an `item_versions` table, giving you a full change log of your data over time.

The project lives in `src/` and is split into six focused modules:

- `cli.rs` — argument parsing
- `main.rs` — top-level orchestration
- `git.rs` — walking Git commit history
- `parser.rs` — parsing JSON and CSV blobs
- `hash.rs` — SHA-256 hashing for change detection
- `processor.rs` — the main processing pipeline
- `db.rs` — SQLite schema and operations

We'll walk through each module in the order that execution flows.

## 1. CLI — `src/cli.rs`

Scribe uses the `clap` crate to define its command-line interface. The `Cli` struct is the single source of truth for every argument the binary accepts.

```bash
cat /home/user/scribe/src/cli.rs
```

```output
use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "scribe",
    version = "1.0.0",
    about = "A high-performance Git history database extractor",
    long_about = "Scribe converts Git commit history of a specific file into a versioned SQLite database. \
                  It's designed to be 10x-100x faster than git-history by using native Git operations \
                  and efficient parallel processing."
)]
pub struct Cli {
    /// Path to the SQLite database file (will be created if it doesn't exist)
    #[arg(value_name = "DB_PATH")]
    pub db_path: PathBuf,

    /// Path to the file in the Git repository to track
    #[arg(value_name = "FILE_IN_GIT")]
    pub file_in_git: String,

    /// Column name(s) that uniquely identify an item (primary key)
    /// Can be specified multiple times for composite keys
    #[arg(short = 'p', long = "primary-key", value_name = "COLUMN", required = true)]
    pub primary_key: Vec<String>,

    /// Path to the Git repository (defaults to current directory)
    #[arg(short = 'r', long = "repo", default_value = ".")]
    pub repo_path: PathBuf,

    /// Verbosity level (-v, -vv, -vvv)
    #[arg(short = 'v', long = "verbose", action = clap::ArgAction::Count)]
    pub verbose: u8,
}

impl Cli {
    pub fn parse_args() -> Self {
        Self::parse()
    }
}
```

Two positional arguments come first: the output database path and the path to the file inside the Git repo to track. The `--primary-key` flag (required, repeatable) tells Scribe which column(s) form a stable identifier for each row — this is what Scribe uses to recognise the same logical item across commits, even if other fields change.

`--repo` defaults to `.`, so running Scribe from inside a repository just works. `--verbose` uses `clap::ArgAction::Count` so you can pass `-v`, `-vv`, or `-vvv` for increasing detail.

## 2. Entry Point — `src/main.rs`

`main()` is the conductor. It calls into every other module in the right order and handles the top-level logic for deciding where to start in the commit history.

```bash
cat /home/user/scribe/src/main.rs
```

```output
mod cli;
mod db;
mod git;
mod hash;
mod parser;
mod processor;

use anyhow::{Context, Result};
use cli::Cli;
use db::Database;
use parser::FileFormat;

fn main() -> Result<()> {
    // Parse command-line arguments
    let args = Cli::parse_args();

    if args.verbose > 0 {
        eprintln!("Scribe v1.0.0 - High-Performance Git History Database Extractor");
        eprintln!("=============================================================");
        eprintln!("Database: {}", args.db_path.display());
        eprintln!("File: {}", args.file_in_git);
        eprintln!("Primary key(s): {}", args.primary_key.join(", "));
        eprintln!("Repository: {}", args.repo_path.display());
        eprintln!();
    }

    // Detect file format
    let file_format = FileFormat::from_path(&args.file_in_git)
        .context("Failed to detect file format")?;

    if args.verbose > 1 {
        eprintln!("Detected format: {:?}", file_format);
    }

    // Open Git repository
    if args.verbose > 0 {
        eprintln!("Opening Git repository...");
    }

    let repo = git::open_repo(&args.repo_path)
        .context("Failed to open Git repository")?;

    if args.verbose > 1 {
        eprintln!("Repository opened successfully");
    }

    // Open or create database
    if args.verbose > 0 {
        eprintln!("Opening database...");
    }

    let mut db = Database::open(&args.db_path)
        .context("Failed to open database")?;

    // Check for last processed commit (for incremental updates)
    let last_processed_commit = db.get_last_processed_commit()?;

    if let Some(ref hash) = last_processed_commit {
        if args.verbose > 0 {
            eprintln!("Found last processed commit: {}", &hash[..8.min(hash.len())]);
            eprintln!("Will process only new commits since then");
        }
    } else {
        if args.verbose > 0 {
            eprintln!("No previous processing found, will process entire history");
        }
    }

    // Walk commit history
    if args.verbose > 0 {
        eprintln!("Walking commit history...");
    }

    let commits = git::walk_history(
        &repo,
        &args.file_in_git,
        last_processed_commit.as_deref(),
        args.verbose,
    )?;

    if commits.is_empty() {
        if args.verbose > 0 {
            eprintln!("No new commits to process. Database is up to date!");
        }
        return Ok(());
    }

    if args.verbose > 0 {
        eprintln!("Found {} commits to process", commits.len());
        eprintln!();
    }

    // Process all commits and build database
    if args.verbose > 0 {
        eprintln!("Processing commits and building database...");
    }

    processor::process_history(
        &mut db,
        commits,
        &args.primary_key,
        file_format,
        args.verbose,
    )?;

    if args.verbose > 0 {
        eprintln!();
        eprintln!("Done! Database written to: {}", args.db_path.display());
    }

    Ok(())
}
```

The flow in `main()` is strictly linear:

1. Parse CLI args.
2. Detect whether the tracked file is JSON or CSV (from its extension).
3. Open the Git repository with `git::open_repo()`.
4. Open (or create) the SQLite database with `Database::open()`.
5. Ask the database for the last commit it already processed — if this is a re-run, Scribe only needs to handle *new* commits.
6. Walk commit history with `git::walk_history()`, passing the optional resume point so the walker can stop early.
7. If there are no new commits, exit immediately — the database is already up to date.
8. Otherwise, hand all commits to `processor::process_history()` which does the heavy lifting.

The `anyhow` crate's `.context()` calls wrap every fallible operation with a human-readable message, so errors bubble up cleanly.

## 3. Git History Walking — `src/git.rs`

This is the performance-critical module. Instead of shelling out to `git log` and reading stdout, Scribe uses the `gix` crate (gitoxide) to open the `.git` object store directly and traverse the commit DAG in-process.

```bash
cat /home/user/scribe/src/git.rs
```

```output
use anyhow::{Context, Result};
use chrono::{DateTime, TimeZone, Utc};
use std::path::Path;

/// Represents a single commit in the history with the file's blob content
#[derive(Debug, Clone)]
pub struct CommitInfo {
    pub hash: String,
    pub timestamp: DateTime<Utc>,
    pub blob_content: Vec<u8>,
}

/// Open a Git repository using gitoxide
pub fn open_repo(path: &Path) -> Result<gix::Repository> {
    gix::discover(path)
        .with_context(|| format!("Failed to open Git repository at {}", path.display()))
}

/// Walk the commit history for a specific file and collect all commits that modified it
/// Returns commits in chronological order (oldest to newest)
pub fn walk_history(
    repo: &gix::Repository,
    file_path: &str,
    last_processed_commit: Option<&str>,
    verbose: u8,
) -> Result<Vec<CommitInfo>> {
    if verbose > 0 {
        eprintln!("Walking commit history for file: {}", file_path);
    }

    // Get HEAD commit
    let head = repo
        .head()?
        .peel_to_commit_in_place()?;

    let head_id = head.id;

    if verbose > 1 {
        eprintln!("Starting from HEAD: {}", head_id);
    }

    // Collect commits that modified the file (in reverse chronological order)
    let mut commits = Vec::new();
    let mut visited = std::collections::HashSet::new();

    // Use a simple queue-based traversal
    let mut queue = vec![head_id];
    visited.insert(head_id);

    while let Some(commit_id) = queue.pop() {
        // Check if we've reached the last processed commit
        if let Some(last_hash) = last_processed_commit {
            if commit_id.to_string() == last_hash {
                if verbose > 0 {
                    eprintln!("Reached last processed commit: {}", last_hash);
                }
                break;
            }
        }

        let commit_obj = repo.find_object(commit_id)?;
        let commit = commit_obj.try_into_commit()?;

        // Get the tree for this commit
        let mut tree = commit.tree()?;

        // Try to find the file in this commit's tree
        let file_in_commit = tree.peel_to_entry_by_path(file_path);

        if let Ok(Some(entry)) = file_in_commit {
            // File exists in this commit, get its blob content
            let blob_obj = entry.object()?;
            let blob = blob_obj.try_into_blob()?;
            let blob_content = blob.data.to_vec();

            // Convert commit time to chrono DateTime
            let timestamp = Utc.timestamp_opt(commit.time()?.seconds, 0)
                .single()
                .context("Invalid timestamp in commit")?;

            commits.push(CommitInfo {
                hash: commit_id.to_string(),
                timestamp,
                blob_content,
            });

            if verbose > 1 {
                eprintln!(
                    "Found file in commit {} at {}",
                    &commit_id.to_string()[..8],
                    timestamp
                );
            }
        }

        // Add parent commits to queue
        for parent_id in commit.parent_ids() {
            let parent_oid: gix::ObjectId = parent_id.into();
            if !visited.contains(&parent_oid) {
                visited.insert(parent_oid);
                queue.push(parent_oid);
            }
        }
    }

    if verbose > 0 {
        eprintln!("Found {} commits that modified the file", commits.len());
    }

    // Reverse to get chronological order (oldest to newest)
    commits.reverse();

    Ok(commits)
}

/// Get the current content of a file at HEAD (for initial processing)
pub fn get_file_at_head(repo: &gix::Repository, file_path: &str) -> Result<Vec<u8>> {
    let head = repo.head()?.peel_to_commit_in_place()?;
    let mut tree = head.tree()?;

    let entry = tree
        .peel_to_entry_by_path(file_path)
        .with_context(|| format!("File '{}' not found in HEAD commit", file_path))?;

    let entry = entry.context("File not found in tree")?;
    let blob_obj = entry.object()?;
    let blob = blob_obj.try_into_blob()?;
    Ok(blob.data.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_open_repo() {
        // This test assumes we're running in a git repository
        let result = open_repo(Path::new("."));
        assert!(result.is_ok(), "Should be able to open the current repository");
    }
}
```

The traversal uses a simple breadth-first queue over the commit DAG:

1. Start at HEAD.
2. Pop a commit, check whether it equals `last_processed_commit` — if so, stop (incremental mode).
3. Look up the tracked file path in that commit's tree. If the file exists, read its blob bytes and record a `CommitInfo`.
4. Push any unseen parent commits onto the queue.

Because Git history can branch and merge, a `HashSet` of visited object IDs prevents the same commit from being processed twice.

The collection ends up in reverse chronological order (HEAD first), so a final `.reverse()` gives oldest-first order for the processor — essential because we want to replay history forward.

`CommitInfo` bundles three things:
- `hash` — the full 40-character commit SHA, used as a foreign key in the database
- `timestamp` — the author time, converted from Unix seconds to a `chrono::DateTime\<Utc\>`
- `blob_content` — the raw bytes of the tracked file at that commit, handed directly to the parser

## 4. Parsing — `src/parser.rs`

The parser turns raw bytes from a Git blob into a list of rows, where each row is a `BTreeMap<String, serde_json::Value>`. `BTreeMap` is used rather than `HashMap` so that keys are always in sorted order — important for deterministic hashing later.

```bash
cat /home/user/scribe/src/parser.rs
```

```output
use anyhow::{Context, Result};
use serde_json::Value;
use std::collections::BTreeMap;

/// Represents the file format
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileFormat {
    Json,
    Csv,
}

impl FileFormat {
    /// Detect the file format from the file path extension
    pub fn from_path(path: &str) -> Result<Self> {
        if path.ends_with(".json") {
            Ok(Self::Json)
        } else if path.ends_with(".csv") {
            Ok(Self::Csv)
        } else {
            anyhow::bail!(
                "Unsupported file format. Only .json and .csv files are supported. Got: {}",
                path
            )
        }
    }
}

/// Parse file contents into a vector of items
pub fn parse_file(
    content: &[u8],
    format: FileFormat,
) -> Result<Vec<BTreeMap<String, Value>>> {
    match format {
        FileFormat::Json => parse_json(content),
        FileFormat::Csv => parse_csv(content),
    }
}

/// Parse JSON content (expects an array of flat objects)
fn parse_json(content: &[u8]) -> Result<Vec<BTreeMap<String, Value>>> {
    let text = std::str::from_utf8(content)
        .context("File content is not valid UTF-8")?;

    let value: Value = serde_json::from_str(text)
        .context("Failed to parse JSON")?;

    match value {
        Value::Array(arr) => {
            let mut items = Vec::new();
            for (idx, item) in arr.into_iter().enumerate() {
                match item {
                    Value::Object(obj) => {
                        // Convert serde_json::Map to BTreeMap for consistent ordering
                        let btree: BTreeMap<String, Value> = obj.into_iter().collect();
                        items.push(btree);
                    }
                    _ => {
                        anyhow::bail!(
                            "JSON array item at index {} is not an object. Expected array of objects.",
                            idx
                        );
                    }
                }
            }
            Ok(items)
        }
        _ => {
            anyhow::bail!(
                "JSON root element must be an array of objects. Got: {}",
                match value {
                    Value::Object(_) => "object",
                    Value::String(_) => "string",
                    Value::Number(_) => "number",
                    Value::Bool(_) => "boolean",
                    Value::Null => "null",
                    _ => "unknown",
                }
            );
        }
    }
}

/// Parse CSV content with a header row
fn parse_csv(content: &[u8]) -> Result<Vec<BTreeMap<String, Value>>> {
    let mut reader = csv::Reader::from_reader(content);

    let headers = reader
        .headers()
        .context("Failed to read CSV headers")?
        .clone();

    let mut items = Vec::new();

    for (idx, result) in reader.records().enumerate() {
        let record = result
            .with_context(|| format!("Failed to read CSV record at row {}", idx + 2))?;

        let mut item = BTreeMap::new();

        for (col_idx, header) in headers.iter().enumerate() {
            let value = record
                .get(col_idx)
                .context("CSV record has fewer columns than headers")?;

            // Try to parse as number or boolean, otherwise keep as string
            let json_value = if let Ok(num) = value.parse::<i64>() {
                Value::Number(num.into())
            } else if let Ok(num) = value.parse::<f64>() {
                Value::Number(
                    serde_json::Number::from_f64(num)
                        .unwrap_or_else(|| serde_json::Number::from(0))
                )
            } else if let Ok(b) = value.parse::<bool>() {
                Value::Bool(b)
            } else if value.is_empty() {
                Value::Null
            } else {
                Value::String(value.to_string())
            };

            item.insert(header.to_string(), json_value);
        }

        items.push(item);
    }

    Ok(items)
}

/// Extract all unique column names from a collection of items
pub fn extract_columns(items: &[BTreeMap<String, Value>]) -> Vec<String> {
    let mut columns = std::collections::HashSet::new();

    for item in items {
        for key in item.keys() {
            columns.insert(key.clone());
        }
    }

    let mut columns: Vec<String> = columns.into_iter().collect();
    columns.sort();
    columns
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_json_array() {
        let json = r#"[
            {"id": 1, "name": "Alice"},
            {"id": 2, "name": "Bob"}
        ]"#;

        let items = parse_json(json.as_bytes()).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].get("name"), Some(&Value::String("Alice".to_string())));
    }

    #[test]
    fn test_parse_csv() {
        let csv = "id,name,age\n1,Alice,30\n2,Bob,25";

        let items = parse_csv(csv.as_bytes()).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].get("name"), Some(&Value::String("Alice".to_string())));
        assert_eq!(items[0].get("age"), Some(&Value::Number(30.into())));
    }

    #[test]
    fn test_extract_columns() {
        let mut item1 = BTreeMap::new();
        item1.insert("id".to_string(), Value::Number(1.into()));
        item1.insert("name".to_string(), Value::String("Alice".to_string()));

        let mut item2 = BTreeMap::new();
        item2.insert("id".to_string(), Value::Number(2.into()));
        item2.insert("age".to_string(), Value::Number(30.into()));

        let items = vec![item1, item2];
        let columns = extract_columns(&items);

        assert_eq!(columns.len(), 3);
        assert!(columns.contains(&"id".to_string()));
        assert!(columns.contains(&"name".to_string()));
        assert!(columns.contains(&"age".to_string()));
    }
}
```

The public surface is small — `FileFormat::from_path()` and `parse_file()` — but the internals are worth understanding:

**JSON path**: The root element must be an array. Each element must be a JSON object. `serde_json::Map` (insertion-ordered) is converted to `BTreeMap` (sorted by key) so that key order is stable for hashing.

**CSV path**: The `csv` crate handles quoting and escaping. Each cell value is then coerced to the most specific type that fits: `i64` → `f64` → `bool` → empty → `String`. This means numeric fields are stored as numbers, not strings, which makes downstream SQL queries more natural.

**`extract_columns()`**: After parsing, the schema is unknown at compile time. This helper scans every row across every commit and collects the union of all column names. The result is sorted so that the order of columns in the database is deterministic across runs.

The sample test data gives a concrete picture of what valid input looks like:

```bash
cat /home/user/scribe/test_data/data.json
```

```output
[
  {"id": 1, "name": "Alice", "age": 31, "city": "New York"},
  {"id": 2, "name": "Bob", "age": 26, "city": "Los Angeles"},
  {"id": 3, "name": "Charlie", "age": 36, "city": "Austin"},
  {"id": 4, "name": "Diana", "age": 29, "city": "Chicago"},
  {"id": 5, "name": "Eve", "age": 27, "city": "Seattle"},
  {"id": 6, "name": "Frank", "age": 32, "city": "Denver"}
]
```

```bash
cat /home/user/scribe/test_data/data.csv
```

```output
id,name,score,grade
101,John,85,B
102,Jane,92,A
103,Mike,78,C
```

## 5. Hashing — `src/hash.rs`

Two SHA-256 hashes drive the entire change-detection system. Both are computed over sorted key-value pairs so that order of fields never matters.

```bash
cat /home/user/scribe/src/hash.rs
```

```output
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

/// Compute a stable hash for a primary key
/// Uses a sorted concatenation of key-value pairs to ensure consistency
pub fn compute_primary_key_hash(
    primary_keys: &[String],
    item: &BTreeMap<String, serde_json::Value>,
) -> anyhow::Result<String> {
    let mut hasher = Sha256::new();

    // Sort keys to ensure consistent ordering
    let mut sorted_keys: Vec<_> = primary_keys.iter().collect();
    sorted_keys.sort();

    for key in sorted_keys {
        if let Some(value) = item.get(key) {
            // Serialize the key-value pair
            hasher.update(key.as_bytes());
            hasher.update(b":");
            hasher.update(value.to_string().as_bytes());
            hasher.update(b";");
        } else {
            anyhow::bail!("Primary key column '{}' not found in item", key);
        }
    }

    Ok(format!("{:x}", hasher.finalize()))
}

/// Compute a hash of the entire item content
/// This is used to detect if an item has changed between commits
pub fn compute_content_hash(item: &BTreeMap<String, serde_json::Value>) -> String {
    let mut hasher = Sha256::new();

    // Sort keys to ensure consistent ordering
    let mut sorted_keys: Vec<_> = item.keys().collect();
    sorted_keys.sort();

    for key in sorted_keys {
        if let Some(value) = item.get(key) {
            hasher.update(key.as_bytes());
            hasher.update(b":");
            hasher.update(value.to_string().as_bytes());
            hasher.update(b";");
        }
    }

    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_primary_key_hash_consistency() {
        let mut item1 = BTreeMap::new();
        item1.insert("id".to_string(), json!(123));
        item1.insert("name".to_string(), json!("test"));

        let mut item2 = BTreeMap::new();
        item2.insert("name".to_string(), json!("test"));
        item2.insert("id".to_string(), json!(123));

        let keys = vec!["id".to_string()];
        let hash1 = compute_primary_key_hash(&keys, &item1).unwrap();
        let hash2 = compute_primary_key_hash(&keys, &item2).unwrap();

        assert_eq!(hash1, hash2, "Hashes should be consistent regardless of insertion order");
    }

    #[test]
    fn test_content_hash_detects_changes() {
        let mut item1 = BTreeMap::new();
        item1.insert("id".to_string(), json!(123));
        item1.insert("value".to_string(), json!("a"));

        let mut item2 = BTreeMap::new();
        item2.insert("id".to_string(), json!(123));
        item2.insert("value".to_string(), json!("b"));

        let hash1 = compute_content_hash(&item1);
        let hash2 = compute_content_hash(&item2);

        assert_ne!(hash1, hash2, "Content hash should change when values change");
    }
}
```

Two hashes serve different roles:

**`compute_primary_key_hash()`** — a *stable identity* for a row across all commits. Given primary keys `["id"]` and a row `{"id": 1, "name": "Alice", "age": 30}`, the hash is computed over just `id:1;`. This hash becomes the `_item_pk` value stored in the database — a short, fixed-width identifier that never changes as the row's other fields evolve.

**`compute_content_hash()`** — a *fingerprint of the whole row*. Computed over all key-value pairs. If this hash differs between two consecutive commits, the row has changed and a new version entry must be written. If it's the same, the commit can be skipped for that row.

Both functions use the same input format: `key:value;` pairs joined together in sorted key order, fed into SHA-256. Sorting is critical — without it, `{"a": 1, "b": 2}` and `{"b": 2, "a": 1}` would hash differently even though they represent the same data.

## 6. Processing Pipeline — `src/processor.rs`

This module is the brain of Scribe. It receives the raw commit list from `git.rs`, orchestrates parallel parsing, extracts the schema, loads existing database state, then processes each commit sequentially to build up the version history.

```bash
cat /home/user/scribe/src/processor.rs
```

```output
use anyhow::{Context, Result};
use rayon::prelude::*;
use std::collections::{BTreeMap, HashMap};

use crate::db::Database;
use crate::git::CommitInfo;
use crate::hash::{compute_content_hash, compute_primary_key_hash};
use crate::parser::{extract_columns, parse_file, FileFormat};

/// Processed data from a single commit
#[derive(Debug)]
struct CommitData {
    hash: String,
    timestamp: chrono::DateTime<chrono::Utc>,
    items: Vec<ItemData>,
}

/// Processed item data with hashes
#[derive(Debug)]
struct ItemData {
    primary_key_hash: String,
    content_hash: String,
    data: BTreeMap<String, serde_json::Value>,
}

/// Main processing function that orchestrates the entire workflow
pub fn process_history(
    db: &mut Database,
    commits: Vec<CommitInfo>,
    primary_keys: &[String],
    file_format: FileFormat,
    verbose: u8,
) -> Result<()> {
    if commits.is_empty() {
        if verbose > 0 {
            eprintln!("No new commits to process");
        }
        return Ok(());
    }

    if verbose > 0 {
        eprintln!("Processing {} commits...", commits.len());
    }

    // Step 1: Parse and hash all commits in parallel
    if verbose > 1 {
        eprintln!("Step 1: Parsing blobs in parallel...");
    }

    let commit_data: Vec<CommitData> = commits
        .par_iter()
        .map(|commit| {
            // Parse the blob content
            let items = parse_file(&commit.blob_content, file_format)
                .with_context(|| {
                    format!(
                        "Failed to parse file content in commit {}",
                        &commit.hash[..8]
                    )
                })?;

            // Compute hashes for each item
            let item_data: Result<Vec<ItemData>> = items
                .into_iter()
                .map(|item| {
                    let pk_hash = compute_primary_key_hash(primary_keys, &item)?;
                    let content_hash = compute_content_hash(&item);

                    Ok(ItemData {
                        primary_key_hash: pk_hash,
                        content_hash,
                        data: item,
                    })
                })
                .collect();

            Ok(CommitData {
                hash: commit.hash.clone(),
                timestamp: commit.timestamp,
                items: item_data?,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    // Step 2: Extract schema from first commit
    if verbose > 1 {
        eprintln!("Step 2: Extracting schema...");
    }

    let all_columns = if let Some(first_commit) = commit_data.first() {
        let sample_items: Vec<BTreeMap<String, serde_json::Value>> = first_commit
            .items
            .iter()
            .map(|item| item.data.clone())
            .collect();
        extract_columns(&sample_items)
    } else {
        vec![]
    };

    // Initialize schema if needed
    db.init_schema(&all_columns)?;

    // Step 3: Load current state from database
    if verbose > 1 {
        eprintln!("Step 3: Loading current state from database...");
    }

    let mut current_state: HashMap<String, String> = db.load_current_state()?;

    if verbose > 1 {
        eprintln!("Loaded {} items from database", current_state.len());
    }

    // Step 4: Process commits sequentially (chronologically)
    if verbose > 1 {
        eprintln!("Step 4: Processing commits and updating database...");
    }

    for (idx, commit) in commit_data.iter().enumerate() {
        if verbose > 0 {
            eprintln!(
                "Processing commit {}/{}: {} ({})",
                idx + 1,
                commit_data.len(),
                &commit.hash[..8],
                commit.timestamp.format("%Y-%m-%d %H:%M:%S")
            );
        }

        // Begin transaction for this commit
        let tx = db.begin_transaction()?;

        // Insert commit record
        Database::insert_commit(&tx, &commit.hash, &commit.timestamp)?;

        let mut new_items = 0;
        let mut changed_items = 0;
        let mut unchanged_items = 0;

        // Process each item in the commit
        for item in &commit.items {
            match current_state.get(&item.primary_key_hash) {
                None => {
                    // New item
                    Database::insert_item(
                        &tx,
                        &item.primary_key_hash,
                        &commit.hash,
                        &all_columns,
                        &item.data,
                    )?;

                    Database::insert_item_version(
                        &tx,
                        &item.primary_key_hash,
                        &commit.hash,
                        &all_columns,
                        &item.data,
                    )?;

                    current_state.insert(
                        item.primary_key_hash.clone(),
                        item.content_hash.clone(),
                    );

                    new_items += 1;
                }
                Some(existing_hash) => {
                    if existing_hash != &item.content_hash {
                        // Item changed
                        Database::update_item(
                            &tx,
                            &item.primary_key_hash,
                            &commit.hash,
                            &all_columns,
                            &item.data,
                        )?;

                        Database::insert_item_version(
                            &tx,
                            &item.primary_key_hash,
                            &commit.hash,
                            &all_columns,
                            &item.data,
                        )?;

                        current_state.insert(
                            item.primary_key_hash.clone(),
                            item.content_hash.clone(),
                        );

                        changed_items += 1;
                    } else {
                        // Item unchanged
                        unchanged_items += 1;
                    }
                }
            }
        }

        if verbose > 1 {
            eprintln!(
                "  New: {}, Changed: {}, Unchanged: {}",
                new_items, changed_items, unchanged_items
            );
        }

        // Update last processed commit within the transaction
        tx.execute(
            "INSERT OR REPLACE INTO _scribe_meta (key, value) VALUES ('last_processed_commit', ?1)",
            rusqlite::params![&commit.hash],
        )?;

        // Commit transaction
        tx.commit()?;
    }

    if verbose > 0 {
        eprintln!("Successfully processed all commits!");
        eprintln!("Total unique items tracked: {}", current_state.len());
    }

    Ok(())
}
```

The four-step pipeline:

**Step 1 — Parallel parsing** uses `rayon::par_iter()` to parse and hash all commit blobs concurrently. Each blob is parsed into rows, and each row gets both its primary key hash and its content hash computed. The result is a `Vec<CommitData>` where the heavy CPU work has already been done. Note the use of `collect::<Result<Vec<_>>>()` — Rayon propagates the first error encountered, cleanly converting parallel errors into the sequential `Result` chain.

**Step 2 — Schema extraction** looks at the first commit only. `extract_columns()` scans the rows and returns a sorted list of column names. These are passed to `db.init_schema()` which creates the tables if they don't already exist.

**Step 3 — State loading** reads the current `items` table into a `HashMap<primary_key_hash, content_hash>`. This in-memory map is the "current truth" — it tells the processor what state each item was in after the last processed commit.

**Step 4 — Sequential commit processing** iterates commits in chronological order. For each commit, a database transaction is opened and each row is classified:
- **Not in `current_state`** → new row: insert into both `items` and `item_versions`
- **In `current_state` but different content hash** → changed row: update `items`, insert into `item_versions`
- **Same content hash** → unchanged: skip

After all rows are processed, `last_processed_commit` is updated inside the same transaction. This is important — if the process crashes mid-commit, the metadata doesn't advance, so the next run will re-process that commit safely.

## 7. Database — `src/db.rs`

All SQLite interaction lives in `db.rs`. The schema is dynamic — column names come from the data — so almost every SQL statement is built at runtime from the column list.

```bash
cat /home/user/scribe/src/db.rs
```

```output
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
```

The four tables and their roles:

**`_scribe_meta`** — a simple key-value store. Currently holds one row: `last_processed_commit`. Checked before history walking to enable incremental updates.

**`commits`** — one row per Git commit hash, with the author timestamp. All rows that touch a commit reference it via foreign keys. `INSERT OR IGNORE` prevents duplicates if the same commit appears in two runs.

**`items`** — one row per unique item (as identified by its primary key hash). Stores the *current* values of the row and `_last_commit_hash` pointing to the most recent commit that changed it. Updated in-place whenever a change is detected.

**`item_versions`** — the append-only history log. One row is written for each (item, commit) pair where the item's content changed (or where the item first appeared). `_version_id AUTOINCREMENT` gives versions a natural ordering independent of timestamp.

**Dynamic SQL construction**: Since the schema is not known at compile time, SQL strings for `INSERT`, `UPDATE`, and `SELECT` are constructed at runtime from the `columns` slice. Column names are always quoted with double quotes to avoid conflicts with SQL keywords.

**`load_current_state()`** is a noteworthy detail: instead of loading a pre-computed hash from the database, it re-reads the column values and recomputes the content hash in Rust. This means the hash algorithm can change between Scribe versions without needing a schema migration — just re-run and the hashes will be recomputed.

## Putting It All Together

Here is the complete data flow from a single `scribe` invocation to a populated database:

```
scribe output.db data.json --primary-key id
        │
        ├─ cli.rs       parse args → { db_path, file_in_git, primary_keys, repo_path }
        │
        ├─ main.rs      detect format (JSON)
        │               open git repo (gix::discover)
        │               open/create database
        │               read last_processed_commit from _scribe_meta
        │
        ├─ git.rs       walk_history()
        │               ╰─ BFS from HEAD, collect blob bytes per commit
        │               ╰─ stop at last_processed_commit (if set)
        │               ╰─ reverse → chronological Vec<CommitInfo>
        │
        └─ processor.rs process_history()
                        │
                        ├─ Step 1  rayon par_iter → parse + hash each commit blob
                        │          Vec<CommitData { hash, timestamp, Vec<ItemData> }>
                        │
                        ├─ Step 2  extract_columns() → sorted column list
                        │          db.init_schema(columns)
                        │
                        ├─ Step 3  db.load_current_state()
                        │          HashMap<pk_hash → content_hash>
                        │
                        └─ Step 4  for each commit (chronological):
                                   begin SQLite transaction
                                   insert commits row
                                   for each item:
                                     pk_hash not in map → INSERT items + item_versions
                                     pk_hash in map, hash differs → UPDATE items + INSERT item_versions
                                     pk_hash in map, hash same  → skip
                                   UPDATE _scribe_meta.last_processed_commit
                                   commit transaction
```

The result is a SQLite file where you can query the current state of any row with `SELECT * FROM items` and see its full change history with `SELECT * FROM item_versions WHERE _item_pk = ?` joined to `commits` for timestamps.

## Key Design Decisions

**Why gitoxide (`gix`) instead of `git2` or shelling out?**  
Gitoxide is a pure-Rust implementation that opens the Git object store directly. No subprocess overhead, no libgit2 linking complexity, and better performance for read-heavy workloads like walking history.

**Why Rayon for parsing?**  
Parsing JSON or CSV is CPU-bound. Rayon's work-stealing thread pool saturates all cores automatically with minimal code change — just swap `iter()` for `par_iter()`. The subsequent commit processing *must* be sequential (history has causal ordering), but parsing is embarrassingly parallel.

**Why SHA-256 for row identity?**  
A composite primary key with arbitrary types could be arbitrarily long and would need careful escaping to use as a SQL identifier. A fixed-width 64-hex-character hash is a clean, safe, and indexable primary key. Collisions are astronomically unlikely for any real dataset.

**Why recompute content hashes from the database?**  
Storing pre-computed hashes would save time on reload, but couples the database format to a specific hash algorithm. Recomputing in `load_current_state()` means the hash logic can evolve without requiring a migration step — just re-run Scribe.

**Why store all data as TEXT in SQLite?**  
SQLite's flexible type system means any value can be stored as TEXT without loss of fidelity for round-tripping. The schema is dynamic, so enforcing strict column types would require complex runtime DDL changes. Querying by number still works because SQLite performs implicit type coercion in `WHERE` clauses.
