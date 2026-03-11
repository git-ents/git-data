mod cli;
mod exe;

use clap::{CommandFactory, Parser};
use cli::{Cli, Command};
use git_metadata::{MetadataIndex, MetadataOptions};
use git2::Oid;
use std::path::PathBuf;
use std::process;

use crate::exe::open_repo;

fn main() {
    if let Some(dir) = parse_generate_man_flag() {
        if let Err(e) = generate_man_page(dir) {
            eprintln!("Error: {}", e);
            process::exit(1);
        }
        return;
    }

    let cli = Cli::parse();

    if let Err(e) = run(&cli) {
        eprintln!("Error: {}", e);
        process::exit(1);
    }
}

fn run(cli: &Cli) -> Result<(), Box<dyn std::error::Error>> {
    let repo = open_repo(cli.repo.as_deref())?;
    let ref_name = &cli.ref_name;

    match &cli.command {
        Command::List => {
            let entries = exe::list(&repo, ref_name)?;
            if entries.is_empty() {
                println!("No entries in {}.", ref_name);
            } else {
                for (target, tree) in &entries {
                    println!("{} {}", target, tree);
                }
            }
        }

        Command::Get { target } => {
            let target_oid = parse_oid(target)?;
            match exe::get(&repo, ref_name, &target_oid)? {
                Some(tree_oid) => println!("{}", tree_oid),
                None => {
                    eprintln!("No metadata entry for {}.", target);
                    process::exit(1);
                }
            }
        }

        Command::Set {
            target,
            tree,
            force,
            shard_level,
        } => {
            let target_oid = parse_oid(target)?;
            let tree_oid = parse_oid(tree)?;
            let opts = MetadataOptions {
                shard_level: *shard_level,
                force: *force,
            };
            let root = exe::set(&repo, ref_name, &target_oid, &tree_oid, &opts)?;
            eprintln!("Set {} -> {} (root tree {}).", target, tree, root);
        }

        Command::Remove { target } => {
            let target_oid = parse_oid(target)?;
            if exe::remove(&repo, ref_name, &target_oid)? {
                eprintln!("Removed metadata entry for {}.", target);
            } else {
                eprintln!("No metadata entry for {}.", target);
                process::exit(1);
            }
        }
    }

    Ok(())
}

fn parse_oid(s: &str) -> Result<Oid, Box<dyn std::error::Error>> {
    Oid::from_str(s).map_err(|e| format!("invalid OID '{}': {}", s, e).into())
}

/// Check for `--generate-man <DIR>` before clap parses, so it doesn't
/// conflict with the required subcommand.
fn parse_generate_man_flag() -> Option<PathBuf> {
    let args: Vec<String> = std::env::args().collect();
    let pos = args.iter().position(|a| a == "--generate-man")?;
    let dir = args
        .get(pos + 1)
        .map(PathBuf::from)
        .unwrap_or_else(default_man_dir);
    Some(dir)
}

fn default_man_dir() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let home = std::env::var_os("HOME").expect("HOME is not set");
            PathBuf::from(home).join(".local/share")
        })
        .join("man")
}

fn generate_man_page(output_dir: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let man1_dir = output_dir.join("man1");
    std::fs::create_dir_all(&man1_dir)?;

    let cmd = Cli::command();
    let man = clap_mangen::Man::new(cmd);
    let mut buffer = Vec::new();
    man.render(&mut buffer)?;

    let man_path = man1_dir.join("git-metadata.1");
    std::fs::write(&man_path, buffer)?;

    let output_dir = output_dir.canonicalize()?;
    eprintln!("Wrote man page to {}", man_path.canonicalize()?.display());

    if !manpath_covers(&output_dir) {
        eprintln!();
        eprintln!("You may need to add this to your shell environment:");
        eprintln!();
        eprintln!("  export MANPATH=\"{}:$MANPATH\"", output_dir.display());
    }
    Ok(())
}

/// Returns `true` if `dir` is equal to, or a subdirectory of, any component
/// in the `MANPATH` environment variable.
fn manpath_covers(dir: &std::path::Path) -> bool {
    let Some(manpath) = std::env::var_os("MANPATH") else {
        return false;
    };
    for component in std::env::split_paths(&manpath) {
        let Ok(component) = component.canonicalize() else {
            continue;
        };
        if dir.starts_with(&component) {
            return true;
        }
    }
    false
}
