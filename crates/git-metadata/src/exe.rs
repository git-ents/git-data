use std::path::Path;

use git2::{Oid, Repository};

use git_metadata::{MetadataEntry, MetadataIndex, MetadataOptions};

/// Open a repository from the given path, or from the environment / current
/// directory when `None` is passed.
pub fn open_repo(path: Option<&Path>) -> Result<Repository, git2::Error> {
    match path {
        Some(p) => Repository::open(p),
        None => Repository::open_from_env(),
    }
}

/// Resolve a revision string (OID, ref, or `HEAD`) to an [`Oid`].
pub fn resolve_oid(repo: &Repository, rev: &str) -> Result<Oid, git2::Error> {
    // Try parsing as a full hex OID first.
    if let Ok(oid) = Oid::from_str(rev) {
        return Ok(oid);
    }
    // Fall back to rev-parse.
    let obj = repo.revparse_single(rev)?;
    Ok(obj.id())
}

/// List all targets that have metadata under `ref_name`.
pub fn list(repo: &Repository, ref_name: &str) -> Result<Vec<(Oid, Oid)>, git2::Error> {
    repo.metadata_list(ref_name)
}

/// Show the metadata tree entries for `target`.
pub fn show(
    repo: &Repository,
    ref_name: &str,
    target: &Oid,
) -> Result<Vec<MetadataEntry>, git2::Error> {
    repo.metadata_show(ref_name, target)
}

/// Add a path entry (with optional content) to a target's metadata tree.
pub fn add(
    repo: &Repository,
    ref_name: &str,
    target: &Oid,
    path: &str,
    content: Option<&[u8]>,
    opts: &MetadataOptions,
) -> Result<Oid, git2::Error> {
    repo.metadata_add(ref_name, target, path, content, opts)
}

/// Remove path entries matching `patterns` from a target's metadata tree.
pub fn remove_paths(
    repo: &Repository,
    ref_name: &str,
    target: &Oid,
    patterns: &[&str],
    keep: bool,
) -> Result<bool, git2::Error> {
    repo.metadata_remove_paths(ref_name, target, patterns, keep)
}

/// Copy metadata from one target to another.
pub fn copy(
    repo: &Repository,
    ref_name: &str,
    from: &Oid,
    to: &Oid,
    opts: &MetadataOptions,
) -> Result<Oid, git2::Error> {
    repo.metadata_copy(ref_name, from, to, opts)
}

/// Remove metadata for targets whose objects no longer exist.
pub fn prune(repo: &Repository, ref_name: &str, dry_run: bool) -> Result<Vec<Oid>, git2::Error> {
    repo.metadata_prune(ref_name, dry_run)
}

/// Return the metadata ref name.
pub fn get_ref(repo: &Repository, ref_name: &str) -> String {
    repo.metadata_get_ref(ref_name)
}

/// Create a bidirectional link between two keys.
pub fn link(
    repo: &Repository,
    ref_name: &str,
    a: &str,
    b: &str,
    forward: &str,
    reverse: &str,
    meta: Option<&[u8]>,
) -> Result<Oid, git2::Error> {
    repo.link(ref_name, a, b, forward, reverse, meta)
}

/// Remove a bidirectional link between two keys.
pub fn unlink(
    repo: &Repository,
    ref_name: &str,
    a: &str,
    b: &str,
    forward: &str,
    reverse: &str,
) -> Result<Oid, git2::Error> {
    repo.unlink(ref_name, a, b, forward, reverse)
}

/// List all links for a key, optionally filtered by relation name.
pub fn linked(
    repo: &Repository,
    ref_name: &str,
    key: &str,
    relation: Option<&str>,
) -> Result<Vec<(String, String)>, git2::Error> {
    repo.linked(ref_name, key, relation)
}
