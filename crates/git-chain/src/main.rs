mod cli;

use clap::Parser;
use cli::{Cli, Command};
use git2::{Oid, Repository};
use git_chain::Chain;
use std::path::Path;
use std::process;

fn main() {
    let cli = Cli::parse();

    if let Err(e) = run(&cli) {
        eprintln!("Error: {}", e);
        process::exit(1);
    }
}

fn open_repo(path: Option<&Path>) -> Result<Repository, git2::Error> {
    match path {
        Some(p) => Repository::open(p),
        None => Repository::open_from_env(),
    }
}

fn run(cli: &Cli) -> Result<(), Box<dyn std::error::Error>> {
    let repo = open_repo(cli.repo.as_deref())?;

    match &cli.command {
        Command::Append {
            ref_name,
            message,
            parent,
            payloads,
        } => {
            let msg = message.as_deref().unwrap_or("chain: append");

            // Build payload tree from files
            let tree = if payloads.is_empty() {
                // Empty tree
                let builder = repo.treebuilder(None)?;
                builder.write()?
            } else {
                let mut entries: Vec<(String, Vec<u8>)> = Vec::new();
                for path in payloads {
                    let name = path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .ok_or("invalid payload path")?
                        .to_string();
                    let content = std::fs::read(path)?;
                    entries.push((name, content));
                }
                let refs: Vec<(&str, &[u8])> = entries
                    .iter()
                    .map(|(k, v)| (k.as_str(), v.as_slice()))
                    .collect();
                repo.build_tree(&refs)?
            };

            let second_parent = match parent {
                Some(s) => Some(Oid::from_str(s)?),
                None => None,
            };

            let entry = repo.append(ref_name, msg, tree, second_parent)?;
            println!("{}", entry.commit);
        }

        Command::Walk { ref_name, thread } => {
            let thread_oid = match thread {
                Some(s) => Some(Oid::from_str(s)?),
                None => None,
            };

            let entries = repo.walk(ref_name, thread_oid)?;
            for entry in &entries {
                println!("{} {}", entry.commit, entry.message);
            }
        }
    }

    Ok(())
}
