use std::path::Path;

use git2::{Oid, Repository};

use crate::{MetadataIndex, MetadataOptions};

/// Open a repository from the given path, or from the environment / current
/// directory when `None` is passed.
pub fn open_repo(path: Option<&Path>) -> Result<Repository, git2::Error> {
    match path {
        Some(p) => Repository::open(p),
        None => Repository::open_from_env(),
    }
}

/// List all entries in the metadata index at `ref_name`.
///
/// Returns `(target_oid, tree_oid)` pairs.  An empty `Vec` means no entries
/// are stored under that ref.
pub fn list(repo: &Repository, ref_name: &str) -> Result<Vec<(Oid, Oid)>, git2::Error> {
    repo.metadata_list(ref_name)
}

/// Read the metadata tree OID attached to `target` under `ref_name`.
///
/// Returns `None` if no entry exists for `target`.
pub fn get(repo: &Repository, ref_name: &str, target: &Oid) -> Result<Option<Oid>, git2::Error> {
    repo.metadata_get(ref_name, target)
}

/// Write or overwrite the metadata tree for `target` under `ref_name`.
///
/// Returns the new root tree OID committed under `ref_name`.
pub fn set(
    repo: &Repository,
    ref_name: &str,
    target: &Oid,
    tree: &Oid,
    opts: &MetadataOptions,
) -> Result<Oid, git2::Error> {
    repo.metadata_set(ref_name, target, tree, opts)
}

/// Remove the metadata entry for `target` under `ref_name`.
///
/// Returns `true` if an entry was removed, `false` if no entry existed.
pub fn remove(repo: &Repository, ref_name: &str, target: &Oid) -> Result<bool, git2::Error> {
    repo.metadata_remove(ref_name, target)
}
