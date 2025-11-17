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

    /// JMESPath expression to transform data (optional)
    #[arg(short = 't', long = "transform")]
    pub transform: Option<String>,
}

impl Cli {
    pub fn parse_args() -> Self {
        Self::parse()
    }
}
