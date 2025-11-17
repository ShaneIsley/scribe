use anyhow::{Context, Result};
use rayon::prelude::*;
use std::collections::{BTreeMap, HashMap};

use crate::db::Database;
use crate::git::CommitInfo;
use crate::hash::{compute_content_hash, compute_primary_key_hash};
use crate::parser::{extract_columns, parse_file_with_transform, FileFormat};

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
    transform: Option<&str>,
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
            let items = parse_file_with_transform(&commit.blob_content, file_format, transform)
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
