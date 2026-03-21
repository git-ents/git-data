//! Append-only event chains stored as Git commit history.
//!
//! A chain is a ref where each commit represents an event. The commit chain
//! provides chronological ordering. Each commit's tree holds only that entry's
//! payload — there is no accumulated state.

use git2::{Error, ErrorCode, Oid, Repository};

/// A single entry in a chain.
#[derive(Debug, Clone)]
pub struct ChainEntry {
    /// The commit OID.
    pub commit: Oid,
    /// The commit message.
    pub message: String,
    /// The tree OID holding the entry's payload.
    pub tree: Oid,
}

/// Core chain operations.
pub trait Chain {
    /// Append a new event to the chain.
    ///
    /// `tree` is a caller-built tree OID holding the event payload.
    /// `parent` is an optional second parent (for threading).
    fn append(
        &self,
        ref_name: &str,
        message: &str,
        tree: Oid,
        parent: Option<Oid>,
    ) -> Result<ChainEntry, Error>;

    /// Walk the chain from tip to root.
    ///
    /// If `thread` is `None`, walks the full chain (first-parent only).
    /// If `thread` is `Some(root)`, returns only commits in that thread.
    fn walk(&self, ref_name: &str, thread: Option<Oid>) -> Result<Vec<ChainEntry>, Error>;

    /// Build a tree from a list of path/content pairs.
    ///
    /// Convenience method for constructing payload trees.
    fn build_tree(&self, entries: &[(&str, &[u8])]) -> Result<Oid, Error>;
}

impl Chain for Repository {
    fn append(
        &self,
        ref_name: &str,
        message: &str,
        tree: Oid,
        parent: Option<Oid>,
    ) -> Result<ChainEntry, Error> {
        let tree_obj = self.find_tree(tree)?;
        let sig = self.signature()?;

        // Get the current tip as the first parent, if the ref exists.
        let tip = match self.find_reference(ref_name) {
            Ok(r) => Some(r.peel_to_commit()?),
            Err(e) if e.code() == ErrorCode::NotFound => None,
            Err(e) => return Err(e),
        };

        let mut parents: Vec<&git2::Commit<'_>> = Vec::new();
        if let Some(ref t) = tip {
            parents.push(t);
        }

        // Add optional second parent.
        let second_parent;
        if let Some(p) = parent {
            second_parent = self.find_commit(p)?;
            parents.push(&second_parent);
        }

        let commit_oid = self.commit(Some(ref_name), &sig, &sig, message, &tree_obj, &parents)?;

        Ok(ChainEntry {
            commit: commit_oid,
            message: message.to_string(),
            tree,
        })
    }

    fn walk(&self, ref_name: &str, thread: Option<Oid>) -> Result<Vec<ChainEntry>, Error> {
        let reference = match self.find_reference(ref_name) {
            Ok(r) => r,
            Err(e) if e.code() == ErrorCode::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e),
        };

        let tip = reference.peel_to_commit()?;

        match thread {
            None => {
                // Full chain walk: follow first-parent links.
                let mut entries = Vec::new();
                let mut current = Some(tip);
                while let Some(commit) = current {
                    entries.push(ChainEntry {
                        commit: commit.id(),
                        message: commit.message().unwrap_or("").to_string(),
                        tree: commit.tree_id(),
                    });
                    current = commit.parent(0).ok();
                }
                Ok(entries)
            }
            Some(root) => {
                // Thread walk: find all commits whose second parent is `root`,
                // then recursively find replies to those.
                // First, collect all commits in the chain.
                let mut all_commits = Vec::new();
                let mut current = Some(tip);
                while let Some(commit) = current {
                    all_commits.push(commit.clone());
                    current = commit.parent(0).ok();
                }

                // Build the thread tree.
                self.collect_thread(&all_commits, root)
            }
        }
    }

    fn build_tree(&self, entries: &[(&str, &[u8])]) -> Result<Oid, Error> {
        build_payload_tree(self, entries)
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a flat tree from path/content pairs.
fn build_payload_tree(repo: &Repository, entries: &[(&str, &[u8])]) -> Result<Oid, Error> {
    let mut builder = repo.treebuilder(None)?;
    for (name, content) in entries {
        let blob_oid = repo.blob(content)?;
        builder.insert(name, blob_oid, 0o100644)?;
    }
    builder.write()
}

/// Helper trait for thread collection (to keep the impl block clean).
trait ThreadWalk {
    fn collect_thread(
        &self,
        all_commits: &[git2::Commit<'_>],
        root: Oid,
    ) -> Result<Vec<ChainEntry>, Error>;
}

impl ThreadWalk for Repository {
    fn collect_thread(
        &self,
        all_commits: &[git2::Commit<'_>],
        root: Oid,
    ) -> Result<Vec<ChainEntry>, Error> {
        let mut result = Vec::new();

        // Include the root commit itself.
        if let Ok(root_commit) = self.find_commit(root) {
            result.push(ChainEntry {
                commit: root_commit.id(),
                message: root_commit.message().unwrap_or("").to_string(),
                tree: root_commit.tree_id(),
            });
        }

        // Find direct replies: commits whose second parent == root.
        let replies: Vec<&git2::Commit<'_>> = all_commits
            .iter()
            .filter(|c| c.parent_id(1).ok() == Some(root))
            .collect();

        for reply in replies {
            // Recursively collect sub-threads.
            let sub = self.collect_thread(all_commits, reply.id())?;
            result.extend(sub);
        }

        Ok(result)
    }
}

#[cfg(test)]
mod tests;
