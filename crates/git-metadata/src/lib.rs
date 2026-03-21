use git_filter_tree::FilterTree as _;
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

/// A single entry in a metadata tree: a path and optional blob content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetadataEntry {
    /// Path relative to the metadata tree root (e.g. `labels/bug`).
    pub path: String,
    /// Blob content, if the entry is a blob. `None` for tree-only entries.
    pub content: Option<Vec<u8>>,
    /// The OID of the entry (blob or tree).
    pub oid: Oid,
    /// Whether this entry is a tree (directory) rather than a blob.
    pub is_tree: bool,
}

/// A metadata index maps [`Oid`] → [`git2::Tree`], stored as a fanout tree
/// under a Git reference (e.g. `refs/metadata/commits`).
///
/// This is analogous to Git notes, which map Oid → Blob, but metadata
/// entries are trees containing arbitrary paths.
pub trait MetadataIndex {
    /// List all targets that have metadata entries.
    /// Returns `(target_oid, tree_oid)` pairs.
    fn metadata_list(&self, ref_name: &str) -> Result<Vec<(Oid, Oid)>, Error>;

    /// Get the raw metadata tree OID for a target.
    /// Returns `None` if no entry exists.
    fn metadata_get(&self, ref_name: &str, target: &Oid) -> Result<Option<Oid>, Error>;

    /// Set the raw metadata tree OID for a target.
    ///
    /// Builds the fanout index tree and returns the new root tree OID.
    /// Does **not** commit; call [`Self::metadata_commit`] to persist.
    fn metadata(
        &self,
        ref_name: &str,
        target: &Oid,
        tree: &Oid,
        opts: &MetadataOptions,
    ) -> Result<Oid, Error>;

    /// Commit a new root tree OID to `ref_name` with the given message.
    ///
    /// Returns the new commit OID.
    fn metadata_commit(&self, ref_name: &str, root: Oid, message: &str) -> Result<Oid, Error>;

    /// Set the raw metadata tree OID for a target.
    /// Returns the new root tree OID committed under `ref_name`.
    ///
    /// # Deprecated
    ///
    /// Use [`Self::metadata`] followed by [`Self::metadata_commit`] instead.
    #[deprecated(since = "0.1.0", note = "use `metadata` + `metadata_commit` instead")]
    fn metadata_set(
        &self,
        ref_name: &str,
        target: &Oid,
        tree: &Oid,
        opts: &MetadataOptions,
    ) -> Result<Oid, Error> {
        #[allow(deprecated)]
        let new_root = self.metadata(ref_name, target, tree, opts)?;
        let msg = format!("metadata: set {} -> {}", target, tree);
        self.metadata_commit(ref_name, new_root, &msg)?;
        Ok(new_root)
    }

    /// Show all entries in the metadata tree for a target.
    /// Returns leaf blob entries with their paths and content.
    fn metadata_show(&self, ref_name: &str, target: &Oid) -> Result<Vec<MetadataEntry>, Error>;

    /// Add a path entry (with optional blob content) to a target's metadata tree.
    ///
    /// If `content` is `Some`, a blob is created at `path`.
    /// If `content` is `None`, an empty blob is created as a marker.
    /// If the target has no metadata yet, a new tree is created.
    /// Errors if the path already exists unless `opts.force` is true.
    fn metadata_add(
        &self,
        ref_name: &str,
        target: &Oid,
        path: &str,
        content: Option<&[u8]>,
        opts: &MetadataOptions,
    ) -> Result<Oid, Error>;

    /// Remove path entries matching `patterns` from a target's metadata tree.
    ///
    /// When `keep` is false, entries matching any pattern are removed.
    /// When `keep` is true, only entries matching a pattern are kept.
    /// Returns `Ok(true)` if anything was removed, `Ok(false)` otherwise.
    fn metadata_remove_paths(
        &self,
        ref_name: &str,
        target: &Oid,
        patterns: &[&str],
        keep: bool,
    ) -> Result<bool, Error>;

    /// Remove the entire metadata entry for a target.
    /// Returns `Ok(true)` if removed, `Ok(false)` if no entry existed.
    fn metadata_remove(&self, ref_name: &str, target: &Oid) -> Result<bool, Error>;

    /// Copy the metadata tree from one target to another.
    /// Errors if `to` already has metadata unless `force` is true.
    /// Errors if `from` has no metadata.
    fn metadata_copy(
        &self,
        ref_name: &str,
        from: &Oid,
        to: &Oid,
        opts: &MetadataOptions,
    ) -> Result<Oid, Error>;

    /// Remove metadata entries for targets that no longer exist in the object database.
    /// Returns the list of pruned target OIDs.
    fn metadata_prune(&self, ref_name: &str, dry_run: bool) -> Result<Vec<Oid>, Error>;

    /// Return the resolved ref name (identity for now, but allows future indirection).
    fn metadata_get_ref(&self, ref_name: &str) -> String;

    /// Create a bidirectional link between two keys.
    ///
    /// Writes `<a>/<forward>/<b>` and `<b>/<reverse>/<a>` in one commit.
    /// `meta` is optional blob content stored at each link entry.
    fn link(
        &self,
        ref_name: &str,
        a: &str,
        b: &str,
        forward: &str,
        reverse: &str,
        meta: Option<&[u8]>,
    ) -> Result<Oid, Error>;

    /// Remove a bidirectional link between two keys.
    ///
    /// Removes `<a>/<forward>/<b>` and `<b>/<reverse>/<a>` in one commit.
    fn unlink(
        &self,
        ref_name: &str,
        a: &str,
        b: &str,
        forward: &str,
        reverse: &str,
    ) -> Result<Oid, Error>;

    /// List all links for a key, optionally filtered by relation name.
    ///
    /// Returns `(relation, target)` pairs.
    fn linked(
        &self,
        ref_name: &str,
        key: &str,
        relation: Option<&str>,
    ) -> Result<Vec<(String, String)>, Error>;

    /// Check whether a specific link exists.
    fn is_linked(&self, ref_name: &str, a: &str, b: &str, forward: &str) -> Result<bool, Error>;
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Split a hex OID string into `(prefix_segments, leaf)` according to `shard_level`.
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

/// Walk into a tree following `segments`, returning the final sub-tree.
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
            let subtree = repo.find_tree(entry.id())?;
            results.extend(collect_entries(repo, &subtree, &full)?);
        } else if let Ok(oid) = Oid::from_str(&full) {
            if oid.to_string() == full {
                results.push((oid, entry.id()));
            }
        }
    }
    Ok(results)
}

/// Detect the fanout path for `target` in `root` by probing all possible depths.
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
fn build_fanout(
    repo: &Repository,
    existing_root: Option<&git2::Tree<'_>>,
    segments: &[String],
    leaf: &str,
    value_tree_oid: &Oid,
) -> Result<Oid, Error> {
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

    let deepest_existing = existing_subtrees.last().and_then(|o| o.as_ref());
    let mut builder = repo.treebuilder(deepest_existing)?;
    builder.insert(leaf, *value_tree_oid, 0o040000)?;
    let mut child_oid = builder.write()?;

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
    NotFound,
    Empty,
    Removed(Oid),
}

/// Build the nested fanout tree for a removal, returning the new root tree OID.
fn build_fanout_remove(
    repo: &Repository,
    root: &git2::Tree<'_>,
    segments: &[String],
    leaf: &str,
) -> Result<RemoveResult, Error> {
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

/// Commit a new root tree under `ref_name`, parenting on the existing commit.
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

/// Recursively collect leaf entries from a metadata tree.
fn collect_tree_entries(
    repo: &Repository,
    tree: &git2::Tree<'_>,
    prefix: &str,
) -> Result<Vec<MetadataEntry>, Error> {
    let mut results = Vec::new();
    for entry in tree.iter() {
        let name = entry.name().unwrap_or("");
        let path = if prefix.is_empty() {
            name.to_string()
        } else {
            format!("{prefix}/{name}")
        };
        match entry.kind() {
            Some(git2::ObjectType::Tree) => {
                let subtree = repo.find_tree(entry.id())?;
                results.extend(collect_tree_entries(repo, &subtree, &path)?);
            }
            Some(git2::ObjectType::Blob) => {
                let blob = repo.find_blob(entry.id())?;
                results.push(MetadataEntry {
                    path,
                    content: Some(blob.content().to_vec()),
                    oid: entry.id(),
                    is_tree: false,
                });
            }
            _ => {}
        }
    }
    Ok(results)
}

/// Insert a blob at `path` within an existing tree (or create a new tree).
/// Path components are split on `/`. Returns the new tree OID.
fn insert_path_into_tree(
    repo: &Repository,
    existing: Option<&git2::Tree<'_>>,
    path: &str,
    blob_oid: Oid,
) -> Result<Oid, Error> {
    let components: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    if components.is_empty() {
        return Err(Error::from_str("empty path"));
    }
    insert_path_recursive(repo, existing, &components, blob_oid)
}

fn insert_path_recursive(
    repo: &Repository,
    existing: Option<&git2::Tree<'_>>,
    components: &[&str],
    blob_oid: Oid,
) -> Result<Oid, Error> {
    assert!(!components.is_empty());

    let name = components[0];

    if components.len() == 1 {
        // Leaf: insert the blob.
        let mut builder = repo.treebuilder(existing)?;
        builder.insert(name, blob_oid, 0o100644)?;
        return builder.write();
    }

    // Intermediate directory: recurse.
    let sub_existing = match existing {
        Some(tree) => match tree.get_name(name) {
            Some(entry) if entry.kind() == Some(git2::ObjectType::Tree) => {
                Some(repo.find_tree(entry.id())?)
            }
            _ => None,
        },
        None => None,
    };

    let child_oid = insert_path_recursive(repo, sub_existing.as_ref(), &components[1..], blob_oid)?;

    let mut builder = repo.treebuilder(existing)?;
    builder.insert(name, child_oid, 0o040000)?;
    builder.write()
}

/// Remove a `/`-separated path from a tree, cleaning up empty parent directories.
/// Returns `None` if the tree becomes empty.
fn remove_path_from_tree(
    repo: &Repository,
    tree: &git2::Tree<'_>,
    path: &str,
) -> Result<Option<Oid>, Error> {
    let components: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    if components.is_empty() {
        return Err(Error::from_str("empty path"));
    }
    remove_path_recursive(repo, tree, &components)
}

fn remove_path_recursive(
    repo: &Repository,
    tree: &git2::Tree<'_>,
    components: &[&str],
) -> Result<Option<Oid>, Error> {
    assert!(!components.is_empty());
    let name = components[0];

    if components.len() == 1 {
        // Leaf: remove the entry.
        let mut builder = repo.treebuilder(Some(tree))?;
        if builder.get(name)?.is_none() {
            return Err(Error::from_str("path not found"));
        }
        builder.remove(name)?;
        if builder.len() == 0 {
            Ok(None)
        } else {
            Ok(Some(builder.write()?))
        }
    } else {
        // Intermediate: recurse into subtree.
        let entry = tree
            .get_name(name)
            .ok_or_else(|| Error::from_str("path not found"))?;
        let subtree = repo.find_tree(entry.id())?;
        let child_oid = remove_path_recursive(repo, &subtree, &components[1..])?;

        let mut builder = repo.treebuilder(Some(tree))?;
        match child_oid {
            Some(oid) => {
                builder.insert(name, oid, 0o040000)?;
            }
            None => {
                builder.remove(name)?;
            }
        }
        if builder.len() == 0 {
            Ok(None)
        } else {
            Ok(Some(builder.write()?))
        }
    }
}

/// Check if a path exists in a tree.
fn path_exists_in_tree(repo: &Repository, tree: &git2::Tree<'_>, path: &str) -> bool {
    let components: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    if components.is_empty() {
        return false;
    }
    path_exists_recursive(repo, tree, &components)
}

fn path_exists_recursive(repo: &Repository, tree: &git2::Tree<'_>, components: &[&str]) -> bool {
    if components.is_empty() {
        return false;
    }
    match tree.get_name(components[0]) {
        None => false,
        Some(entry) => {
            if components.len() == 1 {
                true
            } else if entry.kind() == Some(git2::ObjectType::Tree) {
                match repo.find_tree(entry.id()) {
                    Ok(subtree) => path_exists_recursive(repo, &subtree, &components[1..]),
                    Err(_) => false,
                }
            } else {
                false
            }
        }
    }
}

/// Match a path against a glob-like pattern.
/// Supports `*` (any single component) and `**` (any number of components).
/// Also supports plain prefix matching (e.g. `labels` matches `labels/bug`).
fn glob_matches(pattern: &str, path: &str) -> bool {
    let pat_parts: Vec<&str> = pattern.split('/').filter(|s| !s.is_empty()).collect();
    let path_parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();

    // Exact match shortcut.
    if pattern == path {
        return true;
    }

    // Prefix match: pattern `foo` matches `foo/bar/baz`.
    if !pat_parts.is_empty()
        && !pat_parts.iter().any(|p| *p == "*" || *p == "**")
        && path_parts.starts_with(&pat_parts)
    {
        return true;
    }

    glob_match_recursive(&pat_parts, &path_parts)
}

fn glob_match_recursive(pattern: &[&str], path: &[&str]) -> bool {
    if pattern.is_empty() {
        return path.is_empty();
    }

    if pattern[0] == "**" {
        // `**` matches zero or more components.
        let rest_pat = &pattern[1..];
        for i in 0..=path.len() {
            if glob_match_recursive(rest_pat, &path[i..]) {
                return true;
            }
        }
        return false;
    }

    if path.is_empty() {
        return false;
    }

    let matches_component = pattern[0] == "*" || pattern[0] == path[0];
    if matches_component {
        glob_match_recursive(&pattern[1..], &path[1..])
    } else {
        false
    }
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

    fn metadata(
        &self,
        ref_name: &str,
        target: &Oid,
        tree: &Oid,
        opts: &MetadataOptions,
    ) -> Result<Oid, Error> {
        self.find_tree(*tree)?;

        let (segments, leaf) = shard_oid(target, opts.shard_level);
        let existing_root = resolve_root_tree(self, ref_name)?;

        if !opts.force {
            if let Some(ref root) = existing_root {
                if detect_fanout(self, root, target)?.is_some() {
                    return Err(Error::from_str(
                        "metadata entry already exists (use force to overwrite)",
                    ));
                }
            }
        }

        build_fanout(self, existing_root.as_ref(), &segments, &leaf, tree)
    }

    fn metadata_commit(&self, ref_name: &str, root: Oid, message: &str) -> Result<Oid, Error> {
        commit_index(self, ref_name, root, message)
    }

    fn metadata_show(&self, ref_name: &str, target: &Oid) -> Result<Vec<MetadataEntry>, Error> {
        let root = match resolve_root_tree(self, ref_name)? {
            Some(t) => t,
            None => return Ok(Vec::new()),
        };

        let tree_oid = match detect_fanout(self, &root, target)? {
            Some((_, _, oid)) => oid,
            None => return Ok(Vec::new()),
        };

        let tree = self.find_tree(tree_oid)?;
        collect_tree_entries(self, &tree, "")
    }

    fn metadata_add(
        &self,
        ref_name: &str,
        target: &Oid,
        path: &str,
        content: Option<&[u8]>,
        opts: &MetadataOptions,
    ) -> Result<Oid, Error> {
        let blob_oid = self.blob(content.unwrap_or(b""))?;

        let existing_root = resolve_root_tree(self, ref_name)?;

        // Get existing metadata tree for this target, if any.
        let existing_meta_tree = match &existing_root {
            Some(root) => match detect_fanout(self, root, target)? {
                Some((_, _, oid)) => Some(self.find_tree(oid)?),
                None => None,
            },
            None => None,
        };

        // Check if path already exists.
        if !opts.force {
            if let Some(ref meta_tree) = existing_meta_tree {
                if path_exists_in_tree(self, meta_tree, path) {
                    return Err(Error::from_str(
                        "path already exists in metadata (use --force to overwrite)",
                    ));
                }
            }
        }

        // Build new metadata tree with the path inserted.
        let new_meta_tree_oid =
            insert_path_into_tree(self, existing_meta_tree.as_ref(), path, blob_oid)?;

        // Now set this as the metadata tree for the target.
        let (segments, leaf) = if existing_meta_tree.is_some() {
            // Re-detect to find the current shard layout.
            match &existing_root {
                Some(root) => match detect_fanout(self, root, target)? {
                    Some((s, l, _)) => (s, l),
                    None => shard_oid(target, opts.shard_level),
                },
                None => shard_oid(target, opts.shard_level),
            }
        } else {
            shard_oid(target, opts.shard_level)
        };

        let new_root = build_fanout(
            self,
            existing_root.as_ref(),
            &segments,
            &leaf,
            &new_meta_tree_oid,
        )?;

        let msg = format!("metadata: add {} to {}", path, target);
        commit_index(self, ref_name, new_root, &msg)?;

        Ok(new_meta_tree_oid)
    }

    fn metadata_remove_paths(
        &self,
        ref_name: &str,
        target: &Oid,
        patterns: &[&str],
        keep: bool,
    ) -> Result<bool, Error> {
        let root = match resolve_root_tree(self, ref_name)? {
            Some(t) => t,
            None => return Ok(false),
        };

        let (segments, leaf, meta_oid) = match detect_fanout(self, &root, target)? {
            Some(t) => t,
            None => return Ok(false),
        };

        let meta_tree = self.find_tree(meta_oid)?;
        let patterns_owned: Vec<String> = patterns.iter().map(|s| s.to_string()).collect();
        let new_meta_tree = self.filter_by_predicate(&meta_tree, |_repo, path| {
            let path_str = path.to_str().unwrap_or("");
            let matched = patterns_owned.iter().any(|p| glob_matches(p, path_str));
            if keep { matched } else { !matched }
        })?;

        if new_meta_tree.is_empty() {
            // Metadata tree is now empty — remove the entire entry.
            match build_fanout_remove(self, &root, &segments, &leaf)? {
                RemoveResult::NotFound => Ok(false),
                RemoveResult::Empty => {
                    let mut reference = self.find_reference(ref_name)?;
                    reference.delete()?;
                    Ok(true)
                }
                RemoveResult::Removed(new_root) => {
                    let msg = format!("metadata: remove paths from {}", target);
                    commit_index(self, ref_name, new_root, &msg)?;
                    Ok(true)
                }
            }
        } else if new_meta_tree.id() == meta_oid {
            Ok(false)
        } else {
            let new_root = build_fanout(self, Some(&root), &segments, &leaf, &new_meta_tree.id())?;
            let msg = format!("metadata: remove paths from {}", target);
            commit_index(self, ref_name, new_root, &msg)?;
            Ok(true)
        }
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

    fn metadata_copy(
        &self,
        ref_name: &str,
        from: &Oid,
        to: &Oid,
        opts: &MetadataOptions,
    ) -> Result<Oid, Error> {
        let root = match resolve_root_tree(self, ref_name)? {
            Some(t) => t,
            None => {
                return Err(Error::from_str(&format!(
                    "no metadata entry for source {}",
                    from
                )));
            }
        };

        let source_tree_oid = match detect_fanout(self, &root, from)? {
            Some((_, _, oid)) => oid,
            None => {
                return Err(Error::from_str(&format!(
                    "no metadata entry for source {}",
                    from
                )));
            }
        };

        if !opts.force {
            if detect_fanout(self, &root, to)?.is_some() {
                return Err(Error::from_str(
                    "metadata entry already exists for target (use --force to overwrite)",
                ));
            }
        }

        let (segments, leaf) = shard_oid(to, opts.shard_level);
        let new_root = build_fanout(self, Some(&root), &segments, &leaf, &source_tree_oid)?;

        let msg = format!("metadata: copy {} -> {}", from, to);
        commit_index(self, ref_name, new_root, &msg)?;

        Ok(source_tree_oid)
    }

    fn metadata_prune(&self, ref_name: &str, dry_run: bool) -> Result<Vec<Oid>, Error> {
        let entries = self.metadata_list(ref_name)?;
        let mut pruned = Vec::new();
        let odb = self.odb()?;

        for (target, _) in &entries {
            if !odb.exists(*target) {
                pruned.push(*target);
            }
        }

        if !dry_run {
            for target in &pruned {
                self.metadata_remove(ref_name, target)?;
            }
        }

        Ok(pruned)
    }

    fn metadata_get_ref(&self, ref_name: &str) -> String {
        ref_name.to_string()
    }

    fn link(
        &self,
        ref_name: &str,
        a: &str,
        b: &str,
        forward: &str,
        reverse: &str,
        meta: Option<&[u8]>,
    ) -> Result<Oid, Error> {
        let blob_oid = self.blob(meta.unwrap_or(b""))?;
        let existing_root = resolve_root_tree(self, ref_name)?;

        // Insert a/<forward>/<b>
        let forward_path = format!("{}/{}/{}", a, forward, b);
        let tree1 = insert_path_into_tree(self, existing_root.as_ref(), &forward_path, blob_oid)?;

        // Insert b/<reverse>/<a> into the same tree
        let reverse_path = format!("{}/{}/{}", b, reverse, a);
        let tree1_obj = self.find_tree(tree1)?;
        let tree2 = insert_path_into_tree(self, Some(&tree1_obj), &reverse_path, blob_oid)?;

        let msg = format!("link: {} -[{}]-> {}", a, forward, b);
        commit_index(self, ref_name, tree2, &msg)?;
        Ok(tree2)
    }

    fn unlink(
        &self,
        ref_name: &str,
        a: &str,
        b: &str,
        forward: &str,
        reverse: &str,
    ) -> Result<Oid, Error> {
        let root =
            resolve_root_tree(self, ref_name)?.ok_or_else(|| Error::from_str("ref not found"))?;

        // Remove a/<forward>/<b>
        let forward_path = format!("{}/{}/{}", a, forward, b);
        let tree1 = remove_path_from_tree(self, &root, &forward_path)?
            .ok_or_else(|| Error::from_str("tree became empty after unlink"))?;

        // Remove b/<reverse>/<a>
        let tree1_obj = self.find_tree(tree1)?;
        let reverse_path = format!("{}/{}/{}", b, reverse, a);
        let tree2_opt = remove_path_from_tree(self, &tree1_obj, &reverse_path)?;

        match tree2_opt {
            Some(tree2) => {
                let msg = format!("unlink: {} -[{}]-> {}", a, forward, b);
                commit_index(self, ref_name, tree2, &msg)?;
                Ok(tree2)
            }
            None => {
                // Tree is empty — delete the ref
                let mut reference = self.find_reference(ref_name)?;
                reference.delete()?;
                let empty = self.treebuilder(None)?.write()?;
                Ok(empty)
            }
        }
    }

    fn linked(
        &self,
        ref_name: &str,
        key: &str,
        relation: Option<&str>,
    ) -> Result<Vec<(String, String)>, Error> {
        let root = match resolve_root_tree(self, ref_name)? {
            Some(t) => t,
            None => return Ok(Vec::new()),
        };

        // Find the key's subtree
        let key_entry = match root.get_name(key) {
            Some(e) => e,
            None => return Ok(Vec::new()),
        };
        let key_tree = self.find_tree(key_entry.id())?;

        let mut results = Vec::new();

        if let Some(rel) = relation {
            // Only look at one relation
            if let Some(rel_entry) = key_tree.get_name(rel) {
                if rel_entry.kind() == Some(git2::ObjectType::Tree) {
                    let rel_tree = self.find_tree(rel_entry.id())?;
                    for target_entry in rel_tree.iter() {
                        if let Some(name) = target_entry.name() {
                            results.push((rel.to_string(), name.to_string()));
                        }
                    }
                }
            }
        } else {
            // All relations
            for rel_entry in key_tree.iter() {
                if rel_entry.kind() == Some(git2::ObjectType::Tree) {
                    let rel_name = rel_entry.name().unwrap_or("").to_string();
                    let rel_tree = self.find_tree(rel_entry.id())?;
                    for target_entry in rel_tree.iter() {
                        if let Some(name) = target_entry.name() {
                            results.push((rel_name.clone(), name.to_string()));
                        }
                    }
                }
            }
        }

        Ok(results)
    }

    fn is_linked(&self, ref_name: &str, a: &str, b: &str, forward: &str) -> Result<bool, Error> {
        let root = match resolve_root_tree(self, ref_name)? {
            Some(t) => t,
            None => return Ok(false),
        };
        let path = format!("{}/{}/{}", a, forward, b);
        Ok(path_exists_in_tree(self, &root, &path))
    }
}

#[cfg(test)]
mod tests;
