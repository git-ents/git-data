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

    /// The ref under which metadata is stored (e.g. `refs/metadata/commits`).
    #[arg(short, long, global = true, default_value = "refs/metadata/commits")]
    pub ref_name: String,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(clap::Subcommand)]
pub enum Command {
    /// List all entries in the metadata index.
    List,

    /// Read the metadata tree OID attached to a target object.
    Get {
        /// The OID of the target object to look up.
        target: String,
    },

    /// Write or overwrite the metadata tree for a target object.
    Set {
        /// The OID of the target object.
        target: String,

        /// The OID of the tree to associate with the target.
        tree: String,

        /// Overwrite an existing entry without error.
        #[arg(short, long)]
        force: bool,

        /// Fanout depth: number of 2-hex-char directory segments.
        /// 1 means `ab/cdef01...` (like git-notes). 2 means `ab/cd/ef01...`.
        #[arg(long, default_value_t = 1)]
        shard_level: u8,
    },

    /// Remove the metadata entry for a target object.
    Remove {
        /// The OID of the target object to remove.
        target: String,
    },
}
