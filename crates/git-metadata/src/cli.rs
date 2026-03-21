use std::path::PathBuf;

use clap::Parser;

#[derive(Parser)]
#[command(name = "git metadata", bin_name = "git metadata")]
#[command(
    author,
    version,
    about = "Manage Git object metadata stored in a fanout ref tree.",
    long_about = None
)]
pub struct Cli {
    /// Path to the git repository. Defaults to the current directory.
    #[arg(short = 'C', long, global = true)]
    pub repo: Option<PathBuf>,

    /// The ref under which metadata is stored.
    #[arg(long, global = true, default_value = "refs/metadata/commits")]
    pub r#ref: String,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(clap::Subcommand)]
pub enum Command {
    /// List all targets that have metadata.
    List,

    /// Show the metadata tree entries for an object.
    Show {
        /// The target object (OID or revision). Defaults to HEAD.
        #[arg(default_value = "HEAD")]
        object: String,
    },

    /// Add a path entry to an object's metadata tree.
    Add {
        /// The path to add (e.g. `labels/bug`, `review/status`).
        path: String,

        /// The target object (OID or revision). Defaults to HEAD.
        #[arg(default_value = "HEAD")]
        object: String,

        /// Content to store in the blob. Reads from stdin when omitted.
        #[arg(short, long)]
        message: Option<String>,

        /// Read content from a file.
        #[arg(short = 'F', long, conflicts_with = "message")]
        file: Option<PathBuf>,

        /// Overwrite an existing path without error.
        #[arg(short, long)]
        force: bool,

        /// Allow adding an entry with empty content.
        #[arg(long)]
        allow_empty: bool,

        /// Fanout depth (number of 2-hex-char directory segments).
        #[arg(long, default_value_t = 1)]
        shard_level: u8,
    },

    /// Remove path entries from an object's metadata tree.
    Remove {
        /// Glob patterns for entries to remove (or keep with `--keep`).
        patterns: Vec<String>,

        /// The target object (OID or revision). Defaults to HEAD.
        #[arg(short, long, default_value = "HEAD")]
        object: String,

        /// Invert: keep only entries matching the patterns.
        #[arg(long)]
        keep: bool,
    },

    /// Copy metadata from one object to another.
    Copy {
        /// The source object (OID or revision).
        from: String,

        /// The destination object (OID or revision).
        to: String,

        /// Overwrite existing metadata on the destination.
        #[arg(short, long)]
        force: bool,

        /// Fanout depth (number of 2-hex-char directory segments).
        #[arg(long, default_value_t = 1)]
        shard_level: u8,
    },

    /// Remove metadata for objects that no longer exist.
    Prune {
        /// Only report what would be pruned; do not actually remove.
        #[arg(short = 'n', long)]
        dry_run: bool,

        /// Print each pruned object.
        #[arg(short, long)]
        verbose: bool,
    },

    /// Print the metadata ref name.
    GetRef,

    /// Create a bidirectional link between two keys.
    Link {
        /// The first key (e.g. `issue:42`).
        a: String,
        /// The second key (e.g. `commit:abc123`).
        b: String,
        /// The forward relation label.
        #[arg(long)]
        forward: String,
        /// The reverse relation label.
        #[arg(long)]
        reverse: String,
    },

    /// Remove a bidirectional link between two keys.
    Unlink {
        /// The first key.
        a: String,
        /// The second key.
        b: String,
        /// The forward relation label.
        #[arg(long)]
        forward: String,
        /// The reverse relation label.
        #[arg(long)]
        reverse: String,
    },

    /// List links for a key.
    Linked {
        /// The key to query.
        key: String,
        /// Optional relation filter.
        #[arg(long)]
        relation: Option<String>,
    },
}
