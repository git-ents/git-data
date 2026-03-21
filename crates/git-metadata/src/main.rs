mod cli;
mod exe;

use clap::{CommandFactory, Parser};
use cli::{Cli, Command};
use git_metadata::MetadataOptions;
use std::io::{IsTerminal, Read};
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
    let ref_name = &cli.r#ref;

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

        Command::Show { object } => {
            let target = exe::resolve_oid(&repo, object)?;
            let entries = exe::show(&repo, ref_name, &target)?;
            if entries.is_empty() {
                eprintln!("No metadata for {}.", target);
                process::exit(1);
            }
            for entry in &entries {
                match entry.content.as_deref() {
                    Some(content) if !content.is_empty() => {
                        let text = String::from_utf8_lossy(content);
                        println!("{}\t{}", entry.path, text);
                    }
                    _ => {
                        println!("{}", entry.path);
                    }
                }
            }
        }

        Command::Add {
            path,
            object,
            message,
            file,
            force,
            allow_empty,
            shard_level,
        } => {
            let target = exe::resolve_oid(&repo, object)?;

            let content = if let Some(msg) = message {
                Some(msg.as_bytes().to_vec())
            } else if let Some(filepath) = file {
                Some(std::fs::read(filepath)?)
            } else {
                // Read from stdin if it's not a TTY.
                if atty_stdin() {
                    None
                } else {
                    let mut buf = Vec::new();
                    std::io::stdin().read_to_end(&mut buf)?;
                    Some(buf)
                }
            };

            if !allow_empty {
                if let Some(ref c) = content {
                    if c.is_empty() {
                        return Err("refusing to add empty content (use --allow-empty)".into());
                    }
                }
            }

            let opts = MetadataOptions {
                shard_level: *shard_level,
                force: *force,
            };

            let tree_oid = exe::add(&repo, ref_name, &target, path, content.as_deref(), &opts)?;
            eprintln!("Added {} to {} (tree {}).", path, target, tree_oid);
        }

        Command::Remove {
            patterns,
            object,
            keep,
        } => {
            let target = exe::resolve_oid(&repo, object)?;

            if patterns.is_empty() {
                return Err("at least one pattern is required".into());
            }

            let pat_refs: Vec<&str> = patterns.iter().map(|s| s.as_str()).collect();
            if exe::remove_paths(&repo, ref_name, &target, &pat_refs, *keep)? {
                eprintln!("Removed matching entries from {}.", target);
            } else {
                eprintln!("No matching entries for {}.", target);
                process::exit(1);
            }
        }

        Command::Copy {
            from,
            to,
            force,
            shard_level,
        } => {
            let from_oid = exe::resolve_oid(&repo, from)?;
            let to_oid = exe::resolve_oid(&repo, to)?;
            let opts = MetadataOptions {
                shard_level: *shard_level,
                force: *force,
            };
            let tree_oid = exe::copy(&repo, ref_name, &from_oid, &to_oid, &opts)?;
            eprintln!("Copied {} -> {} (tree {}).", from_oid, to_oid, tree_oid);
        }

        Command::Prune { dry_run, verbose } => {
            let pruned = exe::prune(&repo, ref_name, *dry_run)?;
            if pruned.is_empty() {
                eprintln!("Nothing to prune.");
            } else {
                for oid in &pruned {
                    if *verbose || *dry_run {
                        println!("{}", oid);
                    }
                }
                if *dry_run {
                    eprintln!("{} entries would be pruned.", pruned.len());
                } else {
                    eprintln!("Pruned {} entries.", pruned.len());
                }
            }
        }

        Command::GetRef => {
            println!("{}", exe::get_ref(&repo, ref_name));
        }

        Command::Link {
            a,
            b,
            forward,
            reverse,
        } => {
            let tree_oid = exe::link(&repo, ref_name, a, b, forward, reverse, None)?;
            eprintln!("Linked {} -[{}]-> {} (tree {}).", a, forward, b, tree_oid);
        }

        Command::Unlink {
            a,
            b,
            forward,
            reverse,
        } => {
            let tree_oid = exe::unlink(&repo, ref_name, a, b, forward, reverse)?;
            let _ = tree_oid;
            eprintln!("Unlinked {} -[{}]-> {}.", a, forward, b);
        }

        Command::Linked { key, relation } => {
            let entries = exe::linked(&repo, ref_name, key, relation.as_deref())?;
            if entries.is_empty() {
                eprintln!("No links for {}.", key);
            } else {
                for (rel, target) in &entries {
                    println!("{}\t{}", rel, target);
                }
            }
        }
    }

    Ok(())
}

/// Check if stdin is a terminal (no piped input).
fn atty_stdin() -> bool {
    std::io::stdin().is_terminal()
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
