use std::path::PathBuf;

use clap::Parser;

#[derive(Parser)]
#[command(name = "git ledger", bin_name = "git ledger")]
#[command(
    author,
    version,
    about = "Git-native record storage.",
    long_about = None
)]
pub struct Cli {
    /// Path to the git repository. Defaults to the current directory.
    #[arg(short = 'C', long, global = true)]
    pub repo: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(clap::Subcommand)]
pub enum Command {
    /// Create a new record.
    Create {
        /// The ref prefix (e.g. `refs/issues`).
        ref_prefix: String,

        /// Caller-provided ID. If omitted, uses sequential numbering.
        id: Option<String>,

        /// Use content-addressed ID (hash stdin).
        #[arg(long)]
        content_hash: bool,

        /// Set a field (repeatable). Format: key=value
        #[arg(long = "set", value_name = "KEY=VALUE")]
        fields: Vec<String>,

        /// Commit message.
        #[arg(short, long, default_value = "ledger: create")]
        message: String,
    },

    /// Read a record.
    Read {
        /// The full ref name (e.g. `refs/issues/1`).
        #[arg(name = "ref")]
        ref_name: String,
    },

    /// Update a record.
    Update {
        /// The full ref name.
        #[arg(name = "ref")]
        ref_name: String,

        /// Set a field (repeatable). Format: key=value
        #[arg(long = "set", value_name = "KEY=VALUE")]
        fields: Vec<String>,

        /// Delete a field (repeatable).
        #[arg(long = "delete", value_name = "KEY")]
        deletes: Vec<String>,

        /// Commit message.
        #[arg(short, long, default_value = "ledger: update")]
        message: String,
    },

    /// List all record IDs under a ref prefix.
    List {
        /// The ref prefix (e.g. `refs/issues`).
        ref_prefix: String,
    },

    /// Show the commit history for a record.
    Log {
        /// The full ref name (e.g. `refs/issues/1`).
        #[arg(name = "ref")]
        ref_name: String,
    },
}
