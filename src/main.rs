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
