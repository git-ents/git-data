//! Git-native record storage.
//!
//! Each record is its own ref. The ref points to a commit whose tree holds the
//! record's fields as blobs. Updates create new commits, providing full history.

use git2::{Error, Oid, Repository};

/// A single record in the ledger.
#[derive(Debug, Clone)]
pub struct LedgerEntry {
    /// The record's identifier (e.g. `1`, `abc123`).
    pub id: String,
    /// The full ref name (e.g. `refs/issues/1`).
    pub ref_: String,
    /// The commit OID backing this version of the record.
    pub commit: Oid,
    /// The record's fields as `(name, value)` pairs.
    pub fields: Vec<(String, Vec<u8>)>,
}

/// Strategy for generating record IDs.
pub enum IdStrategy<'a> {
    /// Scan existing refs and use max + 1.
    Sequential,
    /// Hash caller-supplied bytes using git's object hash.
    ContentAddressed(&'a [u8]),
    /// Use the caller's string directly.
    CallerProvided(&'a str),
    /// Name the record's ref after the OID of the commit that `create` writes.
    CommitOid,
}

/// A mutation to apply to a record's fields.
pub enum Mutation<'a> {
    /// Upsert a field.
    Set(&'a str, &'a [u8]),
    /// Delete a field.
    Delete(&'a str),
}

/// Core ledger operations.
pub trait Ledger {
    /// Create a new record under `ref_prefix`.
    fn create(
        &self,
        ref_prefix: &str,
        strategy: &IdStrategy<'_>,
        fields: &[(&str, &[u8])],
        message: &str,
    ) -> Result<LedgerEntry, Error>;

    /// Read an existing record by its full ref name.
    fn read(&self, ref_name: &str) -> Result<LedgerEntry, Error>;

    /// Update an existing record by applying mutations.
    fn update(
        &self,
        ref_name: &str,
        mutations: &[Mutation<'_>],
        message: &str,
    ) -> Result<LedgerEntry, Error>;

    /// List all record IDs under a ref prefix.
    fn list(&self, ref_prefix: &str) -> Result<Vec<String>, Error>;

    /// Return the commit history for a record.
    fn history(&self, ref_name: &str) -> Result<Vec<Oid>, Error>;
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Recursively insert a blob at an arbitrary depth inside a tree builder.
fn insert_nested(
    repo: &Repository,
    builder: &mut git2::TreeBuilder<'_>,
    components: &[&str],
    blob_oid: Oid,
) -> Result<(), Error> {
    match components {
        [leaf] => {
            builder.insert(leaf, blob_oid, 0o100644)?;
        }
        [head, rest @ ..] => {
            let mut sub_builder = if let Some(existing) = builder.get(head)? {
                let existing_tree = repo.find_tree(existing.id())?;
                repo.treebuilder(Some(&existing_tree))?
            } else {
                repo.treebuilder(None)?
            };
            insert_nested(repo, &mut sub_builder, rest, blob_oid)?;
            let sub_tree = sub_builder.write()?;
            builder.insert(head, sub_tree, 0o040000)?;
        }
        [] => {}
    }
    Ok(())
}

/// Recursively remove a blob at an arbitrary depth inside a tree builder.
/// Returns `true` if the subtree at this level is now empty and should be pruned.
fn remove_nested(
    repo: &Repository,
    builder: &mut git2::TreeBuilder<'_>,
    components: &[&str],
) -> Result<bool, Error> {
    match components {
        [leaf] => {
            let _ = builder.remove(leaf);
        }
        [head, rest @ ..] => {
            let existing_tree_id = builder
                .get(head)?
                .filter(|e| e.kind() == Some(git2::ObjectType::Tree))
                .map(|e| e.id());
            if let Some(tree_id) = existing_tree_id {
                let et = repo.find_tree(tree_id)?;
                let mut sub_builder = repo.treebuilder(Some(&et))?;
                let empty = remove_nested(repo, &mut sub_builder, rest)?;
                if empty {
                    let _ = builder.remove(head);
                } else {
                    let sub_tree = sub_builder.write()?;
                    builder.insert(head, sub_tree, 0o040000)?;
                }
            }
        }
        [] => {}
    }
    Ok(builder.is_empty())
}

/// Build a tree from a list of field name/value pairs.
fn build_fields_tree(repo: &Repository, fields: &[(&str, &[u8])]) -> Result<Oid, Error> {
    let mut builder = repo.treebuilder(None)?;
    for (name, value) in fields {
        let blob_oid = repo.blob(value)?;
        let components: Vec<&str> = name.split('/').collect();
        insert_nested(repo, &mut builder, &components, blob_oid)?;
    }
    builder.write()
}

/// Read all fields from a tree (recursively for subdirectories).
fn read_fields(
    repo: &Repository,
    tree: &git2::Tree<'_>,
    prefix: &str,
) -> Result<Vec<(String, Vec<u8>)>, Error> {
    let mut fields = Vec::new();
    for entry in tree.iter() {
        let name = entry.name().unwrap_or("").to_string();
        let path = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{}/{}", prefix, name)
        };
        match entry.kind() {
            Some(git2::ObjectType::Blob) => {
                let blob = repo.find_blob(entry.id())?;
                fields.push((path, blob.content().to_vec()));
            }
            Some(git2::ObjectType::Tree) => {
                let subtree = repo.find_tree(entry.id())?;
                fields.extend(read_fields(repo, &subtree, &path)?);
            }
            _ => {}
        }
    }
    Ok(fields)
}

/// Extract the ID portion from a full ref name given a prefix.
fn id_from_ref(ref_name: &str, ref_prefix: &str) -> String {
    let prefix = if ref_prefix.ends_with('/') {
        ref_prefix.to_string()
    } else {
        format!("{}/", ref_prefix)
    };
    ref_name
        .strip_prefix(&prefix)
        .unwrap_or(ref_name)
        .to_string()
}

/// Generate the next sequential ID by scanning existing refs.
fn next_sequential_id(repo: &Repository, ref_prefix: &str) -> Result<u64, Error> {
    let pattern = if ref_prefix.ends_with('/') {
        format!("{}*", ref_prefix)
    } else {
        format!("{}/*", ref_prefix)
    };
    let refs = repo.references_glob(&pattern)?;
    let mut max_id: u64 = 0;
    for reference in refs {
        let reference = reference?;
        if let Some(name) = reference.name() {
            let id_str = id_from_ref(name, ref_prefix);
            if let Ok(n) = id_str.parse::<u64>() {
                max_id = max_id.max(n);
            }
        }
    }
    Ok(max_id + 1)
}

// ---------------------------------------------------------------------------
// Implementation
// ---------------------------------------------------------------------------

impl Ledger for Repository {
    fn create(
        &self,
        ref_prefix: &str,
        strategy: &IdStrategy<'_>,
        fields: &[(&str, &[u8])],
        message: &str,
    ) -> Result<LedgerEntry, Error> {
        let tree_oid = build_fields_tree(self, fields)?;
        let tree = self.find_tree(tree_oid)?;
        let sig = self.signature()?;

        if let IdStrategy::CommitOid = strategy {
            // Write the commit first (no ref), then name the ref after its OID.
            let commit_oid = self.commit(None, &sig, &sig, message, &tree, &[])?;
            let ref_name = if ref_prefix.ends_with('/') {
                format!("{}{}", ref_prefix, commit_oid)
            } else {
                format!("{}/{}", ref_prefix, commit_oid)
            };
            self.reference(&ref_name, commit_oid, false, message)?;
            let fields = read_fields(self, &tree, "")?;
            return Ok(LedgerEntry {
                id: commit_oid.to_string(),
                ref_: ref_name,
                commit: commit_oid,
                fields,
            });
        }

        let id = match strategy {
            IdStrategy::Sequential => {
                let next = next_sequential_id(self, ref_prefix)?;
                next.to_string()
            }
            IdStrategy::ContentAddressed(bytes) => {
                let oid = self.blob(bytes)?;
                oid.to_string()
            }
            IdStrategy::CallerProvided(s) => s.to_string(),
            IdStrategy::CommitOid => unreachable!(),
        };

        let ref_name = if ref_prefix.ends_with('/') {
            format!("{}{}", ref_prefix, id)
        } else {
            format!("{}/{}", ref_prefix, id)
        };

        // Check the ref doesn't already exist
        if self.find_reference(&ref_name).is_ok() {
            return Err(Error::from_str(&format!(
                "record already exists: {}",
                ref_name
            )));
        }

        let commit_oid = self.commit(
            Some(&ref_name),
            &sig,
            &sig,
            message,
            &tree,
            &[], // no parents for first commit
        )?;

        let fields = read_fields(self, &tree, "")?;
        let id = ref_name.rsplit('/').next().unwrap_or(&ref_name).to_string();

        Ok(LedgerEntry {
            id,
            ref_: ref_name,
            commit: commit_oid,
            fields,
        })
    }

    fn read(&self, ref_name: &str) -> Result<LedgerEntry, Error> {
        let reference = self.find_reference(ref_name)?;
        let commit = reference.peel_to_commit()?;
        let tree = commit.tree()?;
        let fields = read_fields(self, &tree, "")?;

        // Extract ID from ref name — take the last component
        let id = ref_name.rsplit('/').next().unwrap_or(ref_name).to_string();

        Ok(LedgerEntry {
            id,
            ref_: ref_name.to_string(),
            commit: commit.id(),
            fields,
        })
    }

    fn update(
        &self,
        ref_name: &str,
        mutations: &[Mutation<'_>],
        message: &str,
    ) -> Result<LedgerEntry, Error> {
        let reference = self.find_reference(ref_name)?;
        let parent_commit = reference.peel_to_commit()?;
        let existing_tree = parent_commit.tree()?;

        let mut builder = self.treebuilder(Some(&existing_tree))?;

        for mutation in mutations {
            match mutation {
                Mutation::Set(name, value) => {
                    let blob_oid = self.blob(value)?;
                    let components: Vec<&str> = name.split('/').collect();
                    insert_nested(self, &mut builder, &components, blob_oid)?;
                }
                Mutation::Delete(name) => {
                    let components: Vec<&str> = name.split('/').collect();
                    remove_nested(self, &mut builder, &components)?;
                }
            }
        }

        let tree_oid = builder.write()?;
        let tree = self.find_tree(tree_oid)?;
        let sig = self.signature()?;

        let commit_oid = self.commit(
            Some(ref_name),
            &sig,
            &sig,
            message,
            &tree,
            &[&parent_commit],
        )?;

        let fields = read_fields(self, &tree, "")?;
        let id = ref_name.rsplit('/').next().unwrap_or(ref_name).to_string();

        Ok(LedgerEntry {
            id,
            ref_: ref_name.to_string(),
            commit: commit_oid,
            fields,
        })
    }

    fn list(&self, ref_prefix: &str) -> Result<Vec<String>, Error> {
        let pattern = if ref_prefix.ends_with('/') {
            format!("{}*", ref_prefix)
        } else {
            format!("{}/*", ref_prefix)
        };
        let refs = self.references_glob(&pattern)?;
        let mut ids = Vec::new();
        for reference in refs {
            let reference = reference?;
            if let Some(name) = reference.name() {
                ids.push(id_from_ref(name, ref_prefix));
            }
        }
        ids.sort();
        Ok(ids)
    }

    fn history(&self, ref_name: &str) -> Result<Vec<Oid>, Error> {
        let reference = self.find_reference(ref_name)?;
        let commit = reference.peel_to_commit()?;

        let mut oids = Vec::new();
        let mut current = Some(commit);
        while let Some(c) = current {
            oids.push(c.id());
            current = c.parent(0).ok();
        }
        Ok(oids)
    }
}

#[cfg(test)]
mod tests;
