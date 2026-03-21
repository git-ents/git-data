use std::path::PathBuf;

use clap::Parser;

#[derive(Parser)]
#[command(name = "git chain", bin_name = "git chain")]
#[command(
    author,
    version,
    about = "Append-only event chains stored as Git commit history.",
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
    /// Append an event to a chain.
    Append {
        /// The ref name for the chain.
        #[arg(name = "ref")]
        ref_name: String,

        /// Commit message for the event.
        #[arg(short, long)]
        message: Option<String>,

        /// Second parent commit (for threading).
        #[arg(long)]
        parent: Option<String>,

        /// Add a file to the event's payload tree (repeatable).
        #[arg(long = "payload")]
        payloads: Vec<PathBuf>,
    },

    /// Walk a chain from tip to root.
    Walk {
        /// The ref name for the chain.
        #[arg(name = "ref")]
        ref_name: String,

        /// Walk only a specific thread rooted at this commit.
        #[arg(long)]
        thread: Option<String>,
    },
}
