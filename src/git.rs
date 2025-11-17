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
