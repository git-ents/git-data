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

const REF: &str = "refs/test/chain";

fn make_payload(repo: &Repository, content: &str) -> Oid {
    repo.build_tree(&[("data", content.as_bytes())]).unwrap()
}

#[test]
fn append_to_empty_chain() {
    let (_dir, repo) = init_repo();
    let tree = make_payload(&repo, "event 1");

    let entry = repo.append(REF, "first event", tree, None).unwrap();
    assert_eq!(entry.message, "first event");
    assert_eq!(entry.tree, tree);
}

#[test]
fn append_multiple() {
    let (_dir, repo) = init_repo();

    let t1 = make_payload(&repo, "event 1");
    let t2 = make_payload(&repo, "event 2");
    let t3 = make_payload(&repo, "event 3");

    repo.append(REF, "first", t1, None).unwrap();
    repo.append(REF, "second", t2, None).unwrap();
    repo.append(REF, "third", t3, None).unwrap();

    let entries = repo.walk(REF, None).unwrap();
    assert_eq!(entries.len(), 3);
    // Reverse chronological
    assert_eq!(entries[0].message, "third");
    assert_eq!(entries[1].message, "second");
    assert_eq!(entries[2].message, "first");
}

#[test]
fn walk_empty_chain() {
    let (_dir, repo) = init_repo();
    let entries = repo.walk(REF, None).unwrap();
    assert!(entries.is_empty());
}

#[test]
fn append_with_second_parent() {
    let (_dir, repo) = init_repo();

    let t1 = make_payload(&repo, "root");
    let t2 = make_payload(&repo, "reply");

    let root = repo.append(REF, "root event", t1, None).unwrap();
    let reply = repo
        .append(REF, "reply to root", t2, Some(root.commit))
        .unwrap();

    // The reply commit should have 2 parents
    let commit = repo.find_commit(reply.commit).unwrap();
    assert_eq!(commit.parent_count(), 2);
    assert_eq!(commit.parent_id(1).unwrap(), root.commit);
}

#[test]
fn walk_thread() {
    let (_dir, repo) = init_repo();

    let t1 = make_payload(&repo, "post 1");
    let t2 = make_payload(&repo, "post 2");
    let t3 = make_payload(&repo, "reply to post 1");
    let t4 = make_payload(&repo, "unrelated");

    let post1 = repo.append(REF, "post 1", t1, None).unwrap();
    let _post2 = repo.append(REF, "post 2", t2, None).unwrap();
    let _reply1 = repo
        .append(REF, "reply to post 1", t3, Some(post1.commit))
        .unwrap();
    let _unrelated = repo.append(REF, "unrelated", t4, None).unwrap();

    let thread = repo.walk(REF, Some(post1.commit)).unwrap();
    // Should include post1 and its reply
    assert_eq!(thread.len(), 2);
    assert!(thread.iter().any(|e| e.message == "post 1"));
    assert!(thread.iter().any(|e| e.message == "reply to post 1"));
}

#[test]
fn build_tree_helper() {
    let (_dir, repo) = init_repo();
    let tree_oid = repo
        .build_tree(&[("file1", b"content1"), ("file2", b"content2")])
        .unwrap();

    let tree = repo.find_tree(tree_oid).unwrap();
    assert_eq!(tree.len(), 2);
}

#[test]
fn walk_preserves_tree_oids() {
    let (_dir, repo) = init_repo();

    let t1 = make_payload(&repo, "event 1");
    let t2 = make_payload(&repo, "event 2");

    repo.append(REF, "first", t1, None).unwrap();
    repo.append(REF, "second", t2, None).unwrap();

    let entries = repo.walk(REF, None).unwrap();
    assert_eq!(entries[0].tree, t2);
    assert_eq!(entries[1].tree, t1);
}
