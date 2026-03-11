use git2::{Error, ErrorCode, Oid, Repository};

/// Options that control mutating metadata operations.
#[derive(Debug, Clone)]
pub struct MetadataOptions {
    /// Fanout depth (number of 2-hex-char directory segments).
    /// 1 means `ab/cdef01...` (like git-notes), 2 means `ab/cd/ef01...`.
    pub shard_level: u8,
    /// Overwrite an existing entry without error.
    pub force: bool,
}

impl Default for MetadataOptions {
    fn default() -> Self {
        Self {
            shard_level: 1,
            force: false,
        }
    }
}

/// A metadata index maps [`Oid`] → [`git2::Tree`], stored as a fanout tree
/// under a Git reference (e.g. `refs/metadata/commits`).
///
/// This is analogous to Git notes, which map Oid → Blob.
pub trait MetadataIndex {
    /// List all entries in the index.
    /// Returns `(target_oid, tree_oid)` pairs.
    fn metadata_list(&self, ref_name: &str) -> Result<Vec<(Oid, Oid)>, Error>;

    /// Read the metadata tree OID attached to `target`.
    /// Returns `None` if no entry exists.
    fn metadata_get(&self, ref_name: &str, target: &Oid) -> Result<Option<Oid>, Error>;

    /// Write or overwrite the metadata tree for `target`.
    /// Returns the new root tree OID committed under `ref_name`.
    fn metadata_set(
        &self,
        ref_name: &str,
        target: &Oid,
        tree: &Oid,
        opts: &MetadataOptions,
    ) -> Result<Oid, Error>;

    /// Remove the metadata entry for `target`.
    /// The fanout depth is auto-detected from the tree structure.
    /// Returns `Ok(true)` if removed, `Ok(false)` if no entry existed.
    fn metadata_remove(&self, ref_name: &str, target: &Oid) -> Result<bool, Error>;
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Split a hex OID string into `(prefix_segments, leaf)` according to `shard_level`.
/// Each segment is 2 hex chars. `shard_level` is the number of 2-char directory
/// segments; the remainder is the leaf name.
///
/// Example with shard_level=2 and oid `abcdef01...`:
///   segments = ["ab", "cd"], leaf = "ef01..."
fn shard_oid(oid: &Oid, shard_level: u8) -> (Vec<String>, String) {
    let hex = oid.to_string();
    let mut segments = Vec::with_capacity(shard_level as usize);
    let mut pos = 0;
    for _ in 0..shard_level {
        segments.push(hex[pos..pos + 2].to_string());
        pos += 2;
    }
    let leaf = hex[pos..].to_string();
    (segments, leaf)
}

/// Resolve an existing root tree from a reference, if it exists.
fn resolve_root_tree<'r>(
    repo: &'r Repository,
    ref_name: &str,
) -> Result<Option<git2::Tree<'r>>, Error> {
    match repo.find_reference(ref_name) {
        Ok(reference) => {
            let commit = reference.peel_to_commit()?;
            let tree = commit.tree()?;
            Ok(Some(tree))
        }
        Err(e) if e.code() == ErrorCode::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// Walk into a tree following `segments`, returning the final sub-tree (if it exists).
fn walk_tree<'a>(
    repo: &'a Repository,
    root: &git2::Tree<'a>,
    segments: &[String],
) -> Result<Option<git2::Tree<'a>>, Error> {
    let mut current = root.clone();
    for seg in segments {
        let id = match current.get_name(seg) {
            Some(entry) => entry.id(),
            None => return Ok(None),
        };
        current = repo.find_tree(id)?;
    }
    Ok(Some(current))
}

/// Returns `true` if `name` is a 2-char hex string (fanout directory name).
fn is_fanout_segment(name: &str) -> bool {
    name.len() == 2 && name.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Recursively collect all `(target_oid, tree_oid)` entries from a fanout tree.
fn collect_entries(
    repo: &Repository,
    tree: &git2::Tree<'_>,
    prefix: &str,
) -> Result<Vec<(Oid, Oid)>, Error> {
    let mut results = Vec::new();
    for entry in tree.iter() {
        let name = entry.name().unwrap_or("");
        if entry.kind() != Some(git2::ObjectType::Tree) {
            continue;
        }
        let full = format!("{prefix}{name}");
        if is_fanout_segment(name) {
            // Intermediate fanout directory — recurse.
            let subtree = repo.find_tree(entry.id())?;
            // PERF: we're allocating recursively here
            // TODO: change return type
            results.extend(collect_entries(repo, &subtree, &full)?);
        } else if let Ok(oid) = Oid::from_str(&full) {
            // Verify round-trip to guard against zero-padded short parses.
            if oid.to_string() == full {
                results.push((oid, entry.id()));
            }
        }
    }
    Ok(results)
}

/// Detect the fanout path for `target` in `root` by probing all possible depths.
/// Returns `Some((segments, leaf, entry_oid))` if found, `None` otherwise.
fn detect_fanout(
    repo: &Repository,
    root: &git2::Tree<'_>,
    target: &Oid,
) -> Result<Option<(Vec<String>, String, Oid)>, Error> {
    let hex = target.to_string();
    let max_depth = hex.len() / 2;
    for depth in 0..max_depth {
        let prefix_len = depth * 2;
        let segments: Vec<String> = (0..depth)
            .map(|i| hex[i * 2..i * 2 + 2].to_string())
            .collect();
        let leaf = &hex[prefix_len..];

        if let Some(subtree) = walk_tree(repo, root, &segments)? {
            if let Some(entry) = subtree.get_name(leaf) {
                if entry.kind() == Some(git2::ObjectType::Tree) {
                    return Ok(Some((segments, leaf.to_string(), entry.id())));
                }
            }
        }
    }
    Ok(None)
}

/// Build the nested fanout tree for an upsert, returning the new root tree OID.
/// `existing_root` is the current root tree (if any).
fn build_fanout(
    repo: &Repository,
    existing_root: Option<&git2::Tree<'_>>,
    segments: &[String],
    leaf: &str,
    value_tree_oid: &Oid,
) -> Result<Oid, Error> {
    // We build from the leaf back up to the root.
    // First, gather existing sub-trees along the path so we can merge.
    let mut existing_subtrees: Vec<Option<git2::Tree<'_>>> = Vec::new();
    if let Some(root) = existing_root {
        let mut current = Some(root.clone());
        existing_subtrees.push(current.clone());
        for seg in segments {
            current = match &current {
                Some(t) => match t.get_name(seg) {
                    Some(e) => Some(repo.find_tree(e.id())?),
                    None => None,
                },
                None => None,
            };
            existing_subtrees.push(current.clone());
        }
    } else {
        for _ in 0..=segments.len() {
            existing_subtrees.push(None);
        }
    }

    // Build leaf level: insert `leaf -> value_tree_oid` into the deepest directory.
    let deepest_existing = existing_subtrees.last().and_then(|o| o.as_ref());
    let mut builder = repo.treebuilder(deepest_existing)?;
    builder.insert(leaf, *value_tree_oid, 0o040000)?;
    let mut child_oid = builder.write()?;

    // Walk back up through segments.
    for (i, seg) in segments.iter().enumerate().rev() {
        let parent_existing = existing_subtrees[i].as_ref();
        let mut builder = repo.treebuilder(parent_existing)?;
        builder.insert(seg, child_oid, 0o040000)?;
        child_oid = builder.write()?;
    }

    Ok(child_oid)
}

/// Result of a fanout removal operation.
enum RemoveResult {
    /// The entry was not found in the tree.
    NotFound,
    /// The entry was removed; the root tree is now empty.
    Empty,
    /// The entry was removed; here is the new root tree OID.
    Removed(Oid),
}

/// Build the nested fanout tree for a removal, returning the new root tree OID.
fn build_fanout_remove(
    repo: &Repository,
    root: &git2::Tree<'_>,
    segments: &[String],
    leaf: &str,
) -> Result<RemoveResult, Error> {
    // Gather existing sub-tree OIDs along the path, then re-fetch as needed.
    let mut chain_oids: Vec<Oid> = vec![root.id()];
    {
        let mut current = root.clone();
        for seg in segments {
            let id = match current.get_name(seg) {
                Some(e) => e.id(),
                None => return Ok(RemoveResult::NotFound),
            };
            chain_oids.push(id);
            current = repo.find_tree(id)?;
        }
    }

    // Remove the leaf from the deepest tree.
    let deepest = repo.find_tree(*chain_oids.last().unwrap())?;
    let mut builder = repo.treebuilder(Some(&deepest))?;
    if builder.get(leaf)?.is_none() {
        return Ok(RemoveResult::NotFound);
    }
    builder.remove(leaf)?;

    let mut child_oid = if builder.len() == 0 {
        None
    } else {
        Some(builder.write()?)
    };

    // Walk back up.
    for (i, seg) in segments.iter().enumerate().rev() {
        let parent = repo.find_tree(chain_oids[i])?;
        let mut builder = repo.treebuilder(Some(&parent))?;
        match child_oid {
            Some(oid) => {
                builder.insert(seg, oid, 0o040000)?;
            }
            None => {
                builder.remove(seg)?;
            }
        }
        child_oid = if builder.len() == 0 {
            None
        } else {
            Some(builder.write()?)
        };
    }

    match child_oid {
        Some(oid) => Ok(RemoveResult::Removed(oid)),
        None => Ok(RemoveResult::Empty),
    }
}

/// Commit a new root tree under `ref_name`, parenting on the existing commit (if any).
fn commit_index(
    repo: &Repository,
    ref_name: &str,
    tree_oid: Oid,
    message: &str,
) -> Result<Oid, Error> {
    let tree = repo.find_tree(tree_oid)?;
    let sig = repo.signature()?;

    let parent = match repo.find_reference(ref_name) {
        Ok(r) => Some(r.peel_to_commit()?),
        Err(e) if e.code() == ErrorCode::NotFound => None,
        Err(e) => return Err(e),
    };

    let parents: Vec<&git2::Commit<'_>> = parent.iter().collect();

    let commit_oid = repo.commit(Some(ref_name), &sig, &sig, message, &tree, &parents)?;
    Ok(commit_oid)
}

// ---------------------------------------------------------------------------
// Implementation for git2::Repository
// ---------------------------------------------------------------------------

impl MetadataIndex for Repository {
    fn metadata_list(&self, ref_name: &str) -> Result<Vec<(Oid, Oid)>, Error> {
        let root = match resolve_root_tree(self, ref_name)? {
            Some(t) => t,
            None => return Ok(Vec::new()),
        };
        collect_entries(self, &root, "")
    }

    fn metadata_get(&self, ref_name: &str, target: &Oid) -> Result<Option<Oid>, Error> {
        let root = match resolve_root_tree(self, ref_name)? {
            Some(t) => t,
            None => return Ok(None),
        };
        Ok(detect_fanout(self, &root, target)?.map(|(_, _, oid)| oid))
    }

    fn metadata_set(
        &self,
        ref_name: &str,
        target: &Oid,
        tree: &Oid,
        opts: &MetadataOptions,
    ) -> Result<Oid, Error> {
        // Validate that `tree` actually points to a tree object.
        self.find_tree(*tree)?;

        let (segments, leaf) = shard_oid(target, opts.shard_level);

        let existing_root = resolve_root_tree(self, ref_name)?;

        // Check for existing entry when force is false.
        if !opts.force {
            if let Some(ref root) = existing_root {
                if detect_fanout(self, root, target)?.is_some() {
                    return Err(Error::from_str(
                        "metadata entry already exists (use force to overwrite)",
                    ));
                }
            }
        }

        let new_root = build_fanout(self, existing_root.as_ref(), &segments, &leaf, tree)?;

        let msg = format!("metadata: set {} -> {}", target, tree);
        commit_index(self, ref_name, new_root, &msg)?;

        Ok(new_root)
    }

    fn metadata_remove(&self, ref_name: &str, target: &Oid) -> Result<bool, Error> {
        let root = match resolve_root_tree(self, ref_name)? {
            Some(t) => t,
            None => return Ok(false),
        };

        let (segments, leaf) = match detect_fanout(self, &root, target)? {
            Some((segments, leaf, _)) => (segments, leaf),
            None => return Ok(false),
        };

        match build_fanout_remove(self, &root, &segments, &leaf)? {
            RemoveResult::NotFound => Ok(false),
            RemoveResult::Empty => {
                // All entries removed; delete the ref entirely.
                let mut reference = self.find_reference(ref_name)?;
                reference.delete()?;
                Ok(true)
            }
            RemoveResult::Removed(new_root) => {
                let msg = format!("metadata: remove {}", target);
                commit_index(self, ref_name, new_root, &msg)?;
                Ok(true)
            }
        }
    }
}

#[cfg(test)]
mod tests;
