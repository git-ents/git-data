use super::*;
use git2::Repository;

fn init_repo() -> (tempfile::TempDir, Repository) {
    let dir = tempfile::tempdir().unwrap();
    let repo = Repository::init(dir.path()).unwrap();

    let mut config = repo.config().unwrap();
    config.set_str("user.name", "test").unwrap();
    config.set_str("user.email", "test@test").unwrap();

    (dir, repo)
}

fn make_tree(repo: &Repository) -> Oid {
    let blob = repo.blob(b"hello").unwrap();
    let mut builder = repo.treebuilder(None).unwrap();
    builder.insert("file.txt", blob, 0o100644).unwrap();
    builder.write().unwrap()
}

fn make_target(repo: &Repository) -> Oid {
    repo.blob(b"target object").unwrap()
}

const REF: &str = "refs/metadata/test";

// ---- Low-level set/get/list/remove (preserved from original) ----

#[test]
fn set_and_get() {
    let (_dir, repo) = init_repo();
    let target = make_target(&repo);
    let tree_oid = make_tree(&repo);
    let opts = MetadataOptions::default();

    let root = repo.metadata(REF, &target, &tree_oid, &opts).unwrap();
    repo.metadata_commit(REF, root, "metadata: set").unwrap();

    let got = repo.metadata_get(REF, &target).unwrap();
    assert_eq!(got, Some(tree_oid));
}

#[test]
fn get_missing_returns_none() {
    let (_dir, repo) = init_repo();
    let target = make_target(&repo);

    let got = repo.metadata_get(REF, &target).unwrap();
    assert_eq!(got, None);
}

#[test]
fn set_without_force_errors_on_duplicate() {
    let (_dir, repo) = init_repo();
    let target = make_target(&repo);
    let tree_oid = make_tree(&repo);
    let opts = MetadataOptions {
        force: false,
        ..Default::default()
    };

    let root = repo.metadata(REF, &target, &tree_oid, &opts).unwrap();
    repo.metadata_commit(REF, root, "metadata: set").unwrap();
    let result = repo.metadata(REF, &target, &tree_oid, &opts);
    assert!(result.is_err());
}

#[test]
fn set_with_force_overwrites() {
    let (_dir, repo) = init_repo();
    let target = make_target(&repo);
    let tree1 = make_tree(&repo);

    let blob2 = repo.blob(b"other").unwrap();
    let mut b = repo.treebuilder(None).unwrap();
    b.insert("other.txt", blob2, 0o100644).unwrap();
    let tree2 = b.write().unwrap();

    let opts = MetadataOptions {
        force: true,
        ..Default::default()
    };

    let root1 = repo.metadata(REF, &target, &tree1, &opts).unwrap();
    repo.metadata_commit(REF, root1, "metadata: set").unwrap();
    let root2 = repo.metadata(REF, &target, &tree2, &opts).unwrap();
    repo.metadata_commit(REF, root2, "metadata: set").unwrap();

    let got = repo.metadata_get(REF, &target).unwrap();
    assert_eq!(got, Some(tree2));
}

#[test]
fn remove_existing() {
    let (_dir, repo) = init_repo();
    let target = make_target(&repo);
    let tree_oid = make_tree(&repo);

    let root = repo
        .metadata(REF, &target, &tree_oid, &MetadataOptions::default())
        .unwrap();
    repo.metadata_commit(REF, root, "metadata: set").unwrap();

    let removed = repo.metadata_remove(REF, &target).unwrap();
    assert!(removed);

    let got = repo.metadata_get(REF, &target).unwrap();
    assert_eq!(got, None);
}

#[test]
fn remove_nonexistent() {
    let (_dir, repo) = init_repo();
    let target = make_target(&repo);

    let removed = repo.metadata_remove(REF, &target).unwrap();
    assert!(!removed);
}

#[test]
fn list_entries() {
    let (_dir, repo) = init_repo();
    let t1 = repo.blob(b"a").unwrap();
    let t2 = repo.blob(b"b").unwrap();
    let tree1 = make_tree(&repo);

    let blob2 = repo.blob(b"other").unwrap();
    let mut b = repo.treebuilder(None).unwrap();
    b.insert("x.txt", blob2, 0o100644).unwrap();
    let tree2 = b.write().unwrap();

    let opts = MetadataOptions::default();

    let root1 = repo.metadata(REF, &t1, &tree1, &opts).unwrap();
    repo.metadata_commit(REF, root1, "metadata: set").unwrap();
    let root2 = repo.metadata(REF, &t2, &tree2, &opts).unwrap();
    repo.metadata_commit(REF, root2, "metadata: set").unwrap();

    let entries = repo.metadata_list(REF).unwrap();
    assert_eq!(entries.len(), 2);
    assert!(entries.contains(&(t1, tree1)));
    assert!(entries.contains(&(t2, tree2)));
}

#[test]
fn cross_shard_level_get_and_remove() {
    let (_dir, repo) = init_repo();
    let target = make_target(&repo);
    let tree_oid = make_tree(&repo);

    let opts = MetadataOptions {
        shard_level: 3,
        force: false,
    };
    let root = repo.metadata(REF, &target, &tree_oid, &opts).unwrap();
    repo.metadata_commit(REF, root, "metadata: set").unwrap();

    let got = repo.metadata_get(REF, &target).unwrap();
    assert_eq!(got, Some(tree_oid));

    let removed = repo.metadata_remove(REF, &target).unwrap();
    assert!(removed);

    let got = repo.metadata_get(REF, &target).unwrap();
    assert_eq!(got, None);
}

#[test]
fn force_detects_across_shard_levels() {
    let (_dir, repo) = init_repo();
    let target = make_target(&repo);
    let tree_oid = make_tree(&repo);

    let opts2 = MetadataOptions {
        shard_level: 2,
        force: false,
    };
    let root = repo.metadata(REF, &target, &tree_oid, &opts2).unwrap();
    repo.metadata_commit(REF, root, "metadata: set").unwrap();

    let opts1 = MetadataOptions {
        shard_level: 1,
        force: false,
    };
    let result = repo.metadata(REF, &target, &tree_oid, &opts1);
    assert!(result.is_err());
}

// ---- metadata_add ----

#[test]
fn add_creates_entry() {
    let (_dir, repo) = init_repo();
    let target = make_target(&repo);
    let opts = MetadataOptions::default();

    repo.metadata_add(REF, &target, "labels/bug", Some(b""), &opts)
        .unwrap();

    let entries = repo.metadata_show(REF, &target).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].path, "labels/bug");
}

#[test]
fn add_with_content() {
    let (_dir, repo) = init_repo();
    let target = make_target(&repo);
    let opts = MetadataOptions::default();

    repo.metadata_add(REF, &target, "review/status", Some(b"approved"), &opts)
        .unwrap();

    let entries = repo.metadata_show(REF, &target).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].path, "review/status");
    assert_eq!(entries[0].content.as_deref(), Some(b"approved".as_slice()));
}

#[test]
fn add_multiple_paths() {
    let (_dir, repo) = init_repo();
    let target = make_target(&repo);
    let opts = MetadataOptions::default();

    repo.metadata_add(REF, &target, "labels/bug", Some(b""), &opts)
        .unwrap();
    repo.metadata_add(REF, &target, "labels/urgent", Some(b""), &opts)
        .unwrap();
    repo.metadata_add(REF, &target, "review/status", Some(b"pending"), &opts)
        .unwrap();

    let mut entries = repo.metadata_show(REF, &target).unwrap();
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    assert_eq!(entries.len(), 3);
    assert_eq!(entries[0].path, "labels/bug");
    assert_eq!(entries[1].path, "labels/urgent");
    assert_eq!(entries[2].path, "review/status");
}

#[test]
fn add_without_force_errors_on_duplicate_path() {
    let (_dir, repo) = init_repo();
    let target = make_target(&repo);
    let opts = MetadataOptions {
        force: false,
        ..Default::default()
    };

    repo.metadata_add(REF, &target, "labels/bug", Some(b""), &opts)
        .unwrap();
    let result = repo.metadata_add(REF, &target, "labels/bug", Some(b"new"), &opts);
    assert!(result.is_err());
}

#[test]
fn add_with_force_overwrites_path() {
    let (_dir, repo) = init_repo();
    let target = make_target(&repo);
    let opts = MetadataOptions {
        force: true,
        ..Default::default()
    };

    repo.metadata_add(REF, &target, "labels/bug", Some(b"old"), &opts)
        .unwrap();
    repo.metadata_add(REF, &target, "labels/bug", Some(b"new"), &opts)
        .unwrap();

    let entries = repo.metadata_show(REF, &target).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].content.as_deref(), Some(b"new".as_slice()));
}

#[test]
fn add_none_content_creates_empty_blob() {
    let (_dir, repo) = init_repo();
    let target = make_target(&repo);
    let opts = MetadataOptions::default();

    repo.metadata_add(REF, &target, "marker", None, &opts)
        .unwrap();

    let entries = repo.metadata_show(REF, &target).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].path, "marker");
    assert_eq!(entries[0].content.as_deref(), Some(b"".as_slice()));
}

#[test]
fn add_deep_path() {
    let (_dir, repo) = init_repo();
    let target = make_target(&repo);
    let opts = MetadataOptions::default();

    repo.metadata_add(REF, &target, "a/b/c/d", Some(b"deep"), &opts)
        .unwrap();

    let entries = repo.metadata_show(REF, &target).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].path, "a/b/c/d");
    assert_eq!(entries[0].content.as_deref(), Some(b"deep".as_slice()));
}

// ---- metadata_show ----

#[test]
fn show_missing_target_returns_empty() {
    let (_dir, repo) = init_repo();
    let target = make_target(&repo);

    let entries = repo.metadata_show(REF, &target).unwrap();
    assert!(entries.is_empty());
}

#[test]
fn show_missing_ref_returns_empty() {
    let (_dir, repo) = init_repo();
    let target = make_target(&repo);

    let entries = repo
        .metadata_show("refs/metadata/nonexistent", &target)
        .unwrap();
    assert!(entries.is_empty());
}

// ---- metadata_remove_paths ----

#[test]
fn remove_paths_exact_match() {
    let (_dir, repo) = init_repo();
    let target = make_target(&repo);
    let opts = MetadataOptions::default();

    repo.metadata_add(REF, &target, "labels/bug", Some(b""), &opts)
        .unwrap();
    repo.metadata_add(REF, &target, "labels/urgent", Some(b""), &opts)
        .unwrap();

    let removed = repo
        .metadata_remove_paths(REF, &target, &["labels/bug"], false)
        .unwrap();
    assert!(removed);

    let entries = repo.metadata_show(REF, &target).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].path, "labels/urgent");
}

#[test]
fn remove_paths_prefix_match() {
    let (_dir, repo) = init_repo();
    let target = make_target(&repo);
    let opts = MetadataOptions::default();

    repo.metadata_add(REF, &target, "labels/bug", Some(b""), &opts)
        .unwrap();
    repo.metadata_add(REF, &target, "labels/urgent", Some(b""), &opts)
        .unwrap();
    repo.metadata_add(REF, &target, "review/status", Some(b"ok"), &opts)
        .unwrap();

    let removed = repo
        .metadata_remove_paths(REF, &target, &["labels"], false)
        .unwrap();
    assert!(removed);

    let entries = repo.metadata_show(REF, &target).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].path, "review/status");
}

#[test]
fn remove_paths_glob_star() {
    let (_dir, repo) = init_repo();
    let target = make_target(&repo);
    let opts = MetadataOptions::default();

    repo.metadata_add(REF, &target, "labels/bug", Some(b""), &opts)
        .unwrap();
    repo.metadata_add(REF, &target, "labels/urgent", Some(b""), &opts)
        .unwrap();
    repo.metadata_add(REF, &target, "review/status", Some(b"ok"), &opts)
        .unwrap();

    let removed = repo
        .metadata_remove_paths(REF, &target, &["labels/*"], false)
        .unwrap();
    assert!(removed);

    let entries = repo.metadata_show(REF, &target).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].path, "review/status");
}

#[test]
fn remove_paths_glob_doublestar() {
    let (_dir, repo) = init_repo();
    let target = make_target(&repo);
    let opts = MetadataOptions::default();

    repo.metadata_add(REF, &target, "a/b/c", Some(b""), &opts)
        .unwrap();
    repo.metadata_add(REF, &target, "a/x", Some(b""), &opts)
        .unwrap();
    repo.metadata_add(REF, &target, "z", Some(b""), &opts)
        .unwrap();

    let removed = repo
        .metadata_remove_paths(REF, &target, &["**/c"], false)
        .unwrap();
    assert!(removed);

    let mut entries = repo.metadata_show(REF, &target).unwrap();
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].path, "a/x");
    assert_eq!(entries[1].path, "z");
}

#[test]
fn remove_paths_keep_mode() {
    let (_dir, repo) = init_repo();
    let target = make_target(&repo);
    let opts = MetadataOptions::default();

    repo.metadata_add(REF, &target, "labels/bug", Some(b""), &opts)
        .unwrap();
    repo.metadata_add(REF, &target, "labels/urgent", Some(b""), &opts)
        .unwrap();
    repo.metadata_add(REF, &target, "review/status", Some(b"ok"), &opts)
        .unwrap();

    // Keep only labels/* entries, remove everything else.
    let removed = repo
        .metadata_remove_paths(REF, &target, &["labels/*"], true)
        .unwrap();
    assert!(removed);

    let mut entries = repo.metadata_show(REF, &target).unwrap();
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].path, "labels/bug");
    assert_eq!(entries[1].path, "labels/urgent");
}

#[test]
fn remove_paths_no_match_returns_false() {
    let (_dir, repo) = init_repo();
    let target = make_target(&repo);
    let opts = MetadataOptions::default();

    repo.metadata_add(REF, &target, "labels/bug", Some(b""), &opts)
        .unwrap();

    let removed = repo
        .metadata_remove_paths(REF, &target, &["nonexistent"], false)
        .unwrap();
    assert!(!removed);
}

#[test]
fn remove_paths_all_removes_entire_entry() {
    let (_dir, repo) = init_repo();
    let target = make_target(&repo);
    let opts = MetadataOptions::default();

    repo.metadata_add(REF, &target, "labels/bug", Some(b""), &opts)
        .unwrap();

    let removed = repo
        .metadata_remove_paths(REF, &target, &["labels/bug"], false)
        .unwrap();
    assert!(removed);

    // Target should have no metadata at all now.
    let got = repo.metadata_get(REF, &target).unwrap();
    assert_eq!(got, None);
}

#[test]
fn remove_paths_missing_target_returns_false() {
    let (_dir, repo) = init_repo();
    let target = make_target(&repo);

    let removed = repo
        .metadata_remove_paths(REF, &target, &["anything"], false)
        .unwrap();
    assert!(!removed);
}

// ---- metadata_copy ----

#[test]
fn copy_metadata() {
    let (_dir, repo) = init_repo();
    let from = repo.blob(b"source").unwrap();
    let to = repo.blob(b"dest").unwrap();
    let opts = MetadataOptions::default();

    repo.metadata_add(REF, &from, "labels/bug", Some(b""), &opts)
        .unwrap();
    repo.metadata_add(REF, &from, "review/status", Some(b"ok"), &opts)
        .unwrap();

    repo.metadata_copy(REF, &from, &to, &opts).unwrap();

    let from_entries = repo.metadata_show(REF, &from).unwrap();
    let to_entries = repo.metadata_show(REF, &to).unwrap();

    assert_eq!(from_entries.len(), to_entries.len());
    for (f, t) in from_entries.iter().zip(to_entries.iter()) {
        assert_eq!(f.path, t.path);
        assert_eq!(f.content, t.content);
    }
}

#[test]
fn copy_errors_when_source_missing() {
    let (_dir, repo) = init_repo();
    let from = repo.blob(b"source").unwrap();
    let to = repo.blob(b"dest").unwrap();
    let opts = MetadataOptions::default();

    let result = repo.metadata_copy(REF, &from, &to, &opts);
    assert!(result.is_err());
}

#[test]
fn copy_errors_when_dest_exists_without_force() {
    let (_dir, repo) = init_repo();
    let from = repo.blob(b"source").unwrap();
    let to = repo.blob(b"dest").unwrap();
    let opts = MetadataOptions {
        force: false,
        ..Default::default()
    };

    repo.metadata_add(REF, &from, "x", Some(b""), &opts)
        .unwrap();
    repo.metadata_add(REF, &to, "y", Some(b""), &opts).unwrap();

    let result = repo.metadata_copy(REF, &from, &to, &opts);
    assert!(result.is_err());
}

#[test]
fn copy_with_force_overwrites_dest() {
    let (_dir, repo) = init_repo();
    let from = repo.blob(b"source").unwrap();
    let to = repo.blob(b"dest").unwrap();
    let opts_no_force = MetadataOptions::default();

    repo.metadata_add(REF, &from, "labels/bug", Some(b"from"), &opts_no_force)
        .unwrap();
    repo.metadata_add(REF, &to, "old/entry", Some(b"old"), &opts_no_force)
        .unwrap();

    let opts_force = MetadataOptions {
        force: true,
        ..Default::default()
    };
    repo.metadata_copy(REF, &from, &to, &opts_force).unwrap();

    let entries = repo.metadata_show(REF, &to).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].path, "labels/bug");
}

// ---- metadata_prune ----

#[test]
fn prune_dry_run_does_not_remove() {
    let (_dir, repo) = init_repo();
    let target = make_target(&repo);
    let opts = MetadataOptions::default();

    repo.metadata_add(REF, &target, "labels/bug", Some(b""), &opts)
        .unwrap();

    // target is a valid blob, so prune should find nothing.
    let pruned = repo.metadata_prune(REF, true).unwrap();
    assert!(pruned.is_empty());

    // Metadata is still there.
    let entries = repo.metadata_show(REF, &target).unwrap();
    assert_eq!(entries.len(), 1);
}

#[test]
fn prune_empty_ref() {
    let (_dir, repo) = init_repo();
    let pruned = repo.metadata_prune(REF, false).unwrap();
    assert!(pruned.is_empty());
}

// ---- metadata_get_ref ----

#[test]
fn get_ref_returns_ref_name() {
    let (_dir, repo) = init_repo();
    assert_eq!(repo.metadata_get_ref(REF), REF);
    assert_eq!(
        repo.metadata_get_ref("refs/metadata/custom"),
        "refs/metadata/custom"
    );
}

// ---- glob_matches (unit tests for the helper) ----

#[test]
fn glob_exact_match() {
    assert!(glob_matches("labels/bug", "labels/bug"));
    assert!(!glob_matches("labels/bug", "labels/urgent"));
}

#[test]
fn glob_prefix_match() {
    assert!(glob_matches("labels", "labels/bug"));
    assert!(glob_matches("labels", "labels/sub/deep"));
    assert!(!glob_matches("labels", "review/status"));
}

#[test]
fn glob_single_star() {
    assert!(glob_matches("labels/*", "labels/bug"));
    assert!(glob_matches("labels/*", "labels/urgent"));
    assert!(!glob_matches("labels/*", "labels/sub/deep"));
    assert!(!glob_matches("labels/*", "review/status"));
}

#[test]
fn glob_double_star() {
    assert!(glob_matches("**", "anything"));
    assert!(glob_matches("**", "a/b/c"));
    assert!(glob_matches("**/bug", "labels/bug"));
    assert!(glob_matches("**/bug", "deep/nested/bug"));
    assert!(!glob_matches("**/bug", "labels/urgent"));
    assert!(glob_matches("a/**/d", "a/b/c/d"));
    assert!(glob_matches("a/**/d", "a/d"));
}

#[test]
fn glob_no_false_positives() {
    assert!(!glob_matches("review", "labels/bug"));
    assert!(!glob_matches("review/*", "labels/bug"));
}

// ---- Relation (link/unlink/linked/is_linked) ----

#[test]
fn link_creates_bidirectional() {
    let (_dir, repo) = init_repo();
    repo.link(REF, "issue:1", "commit:abc", "fixes", "fixed-by", None)
        .unwrap();

    assert!(
        repo.is_linked(REF, "issue:1", "commit:abc", "fixes")
            .unwrap()
    );
    assert!(
        repo.is_linked(REF, "commit:abc", "issue:1", "fixed-by")
            .unwrap()
    );
}

#[test]
fn link_with_metadata() {
    let (_dir, repo) = init_repo();
    repo.link(
        REF,
        "issue:1",
        "commit:abc",
        "fixes",
        "fixed-by",
        Some(b"meta"),
    )
    .unwrap();

    let links = repo.linked(REF, "issue:1", Some("fixes")).unwrap();
    assert_eq!(links.len(), 1);
    assert_eq!(links[0], ("fixes".to_string(), "commit:abc".to_string()));
}

#[test]
fn unlink_removes_both_directions() {
    let (_dir, repo) = init_repo();
    repo.link(REF, "issue:1", "commit:abc", "fixes", "fixed-by", None)
        .unwrap();
    repo.unlink(REF, "issue:1", "commit:abc", "fixes", "fixed-by")
        .unwrap();

    assert!(
        !repo
            .is_linked(REF, "issue:1", "commit:abc", "fixes")
            .unwrap()
    );
    assert!(
        !repo
            .is_linked(REF, "commit:abc", "issue:1", "fixed-by")
            .unwrap()
    );
}

#[test]
fn linked_returns_all_relations() {
    let (_dir, repo) = init_repo();
    repo.link(REF, "issue:1", "commit:abc", "fixes", "fixed-by", None)
        .unwrap();
    repo.link(REF, "issue:1", "commit:def", "fixes", "fixed-by", None)
        .unwrap();
    repo.link(REF, "issue:1", "pr:10", "closes", "closed-by", None)
        .unwrap();

    let all = repo.linked(REF, "issue:1", None).unwrap();
    assert_eq!(all.len(), 3);

    let fixes: Vec<_> = all.iter().filter(|(r, _)| r == "fixes").collect();
    assert_eq!(fixes.len(), 2);

    let closes: Vec<_> = all.iter().filter(|(r, _)| r == "closes").collect();
    assert_eq!(closes.len(), 1);
}

#[test]
fn linked_filters_by_relation() {
    let (_dir, repo) = init_repo();
    repo.link(REF, "issue:1", "commit:abc", "fixes", "fixed-by", None)
        .unwrap();
    repo.link(REF, "issue:1", "pr:10", "closes", "closed-by", None)
        .unwrap();

    let fixes = repo.linked(REF, "issue:1", Some("fixes")).unwrap();
    assert_eq!(fixes.len(), 1);
    assert_eq!(fixes[0].1, "commit:abc");
}

#[test]
fn is_linked_returns_false_for_missing() {
    let (_dir, repo) = init_repo();
    assert!(
        !repo
            .is_linked(REF, "issue:1", "commit:abc", "fixes")
            .unwrap()
    );
}

#[test]
fn linked_empty_ref_returns_empty() {
    let (_dir, repo) = init_repo();
    let result = repo.linked(REF, "issue:1", None).unwrap();
    assert!(result.is_empty());
}
