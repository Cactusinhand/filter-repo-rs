mod common;
use common::*;

#[test]
fn tag_rename_lightweight_creates_new_and_deletes_old() {
    let repo = init_repo();
    assert_eq!(run_git(&repo, &["tag", "v1.0"]).0, 0);
    let (_c, _o, _e) = run_tool(&repo, |o| {
        o.no_data = true;
        o.tag_rename = Some((b"v".to_vec(), b"release-".to_vec()));
    });
    let (_c2, out, _e2) = run_git(&repo, &["show-ref", "--tags"]);
    assert!(out.contains("refs/tags/release-1.0"));
    assert!(!out.contains("refs/tags/v1.0"));
}

#[test]
fn tag_rename_annotated_produces_tag_object() {
    let repo = init_repo();
    assert_eq!(run_git(&repo, &["tag", "-a", "-m", "hello tag", "v1.0"]).0, 0);
    let (_c, _o, _e) = run_tool(&repo, |o| {
        o.no_data = true;
        o.tag_rename = Some((b"v".to_vec(), b"release-".to_vec()));
    });
    let (_c1, oid, _e1) = run_git(&repo, &["rev-parse", "refs/tags/release-1.0"]);
    let oid = oid.trim();
    let (_c2, typ, _e2) = run_git(&repo, &["cat-file", "-t", oid]);
    assert_eq!(typ.trim(), "tag");
}

#[test]
fn branch_rename_updates_ref_and_head() {
    let repo = init_repo();
    let (_c0, headref, _e0) = run_git(&repo, &["symbolic-ref", "HEAD"]);
    let headref = headref.trim().to_string();
    let (_c, _o, _e) = run_tool(&repo, |o| {
        o.branch_rename = Some((Vec::new(), b"renamed-".to_vec()));
        o.no_data = true;
    });
    let orig_name = headref.strip_prefix("refs/heads/").unwrap_or(&headref);
    let new_branch = format!("refs/heads/renamed-{}", orig_name);
    let (_c1, out1, _e1) = run_git(&repo, &["show-ref", "--verify", &new_branch]);
    assert!(!out1.is_empty());
    let (_c2, out2, _e2) = run_git(&repo, &["show-ref", "--verify", &headref]);
    assert!(out2.is_empty());
    let (_c3, head_after, _e3) = run_git(&repo, &["symbolic-ref", "HEAD"]);
    assert_eq!(head_after.trim(), new_branch);
}

#[test]
fn branch_rename_without_new_commits_updates_refs() {
    let repo = init_repo();
    assert_eq!(run_git(&repo, &["checkout", "-q", "-b", "feature/plain"]).0, 0);
    let (_c_before, head_before, _e_before) = run_git(&repo, &["symbolic-ref", "HEAD"]);
    assert_eq!(head_before.trim(), "refs/heads/feature/plain");
    let (_c, _o, _e) = run_tool(&repo, |o| {
        o.branch_rename = Some((b"feature/".to_vec(), b"topic/".to_vec()));
        o.no_data = true;
    });
    let (_c_new, out_new, _e_new) = run_git(&repo, &["show-ref", "--verify", "refs/heads/topic/plain"]);
    assert!(!out_new.is_empty());
    let (_c_old, out_old, _e_old) = run_git(&repo, &["show-ref", "--verify", "refs/heads/feature/plain"]);
    assert!(out_old.is_empty());
    let (_c_head, head_after, _e_head) = run_git(&repo, &["symbolic-ref", "HEAD"]);
    assert_eq!(head_after.trim(), "refs/heads/topic/plain");
}

#[test]
fn branch_prefix_rename_preserves_head_to_mapped_target() {
    let repo = init_repo();
    assert_eq!(run_git(&repo, &["checkout", "-q", "-b", "features/foo"]).0, 0);
    write_file(&repo, "feat.txt", "feat");
    run_git(&repo, &["add", "."]).0;
    assert_eq!(run_git(&repo, &["commit", "-q", "-m", "feat commit"]).0, 0);
    let (_c0, headref, _e0) = run_git(&repo, &["symbolic-ref", "HEAD"]);
    assert_eq!(headref.trim(), "refs/heads/features/foo");
    let (_c, _o, _e) = run_tool(&repo, |o| {
        o.branch_rename = Some((b"features/".to_vec(), b"topics/".to_vec()));
        o.no_data = true;
    });
    let (_c3, head_after, _e3) = run_git(&repo, &["symbolic-ref", "HEAD"]);
    assert_eq!(head_after.trim(), "refs/heads/topics/foo");
}

#[test]
fn head_preserved_when_branch_unchanged() {
    let repo = init_repo();
    let (_c0, headref, _e0) = run_git(&repo, &["symbolic-ref", "HEAD"]);
    let headref = headref.trim().to_string();
    assert!(headref.starts_with("refs/heads/"));
    assert_eq!(run_git(&repo, &["checkout", "-q", "-b", "feature/x"]).0, 0);
    assert_eq!(
        run_git(
            &repo,
            &[
                "checkout",
                "-q",
                headref.strip_prefix("refs/heads/").unwrap_or(&headref),
            ],
        )
        .0,
        0
    );
    let (_c, _o, _e) = run_tool(&repo, |o| {
        o.branch_rename = Some((b"feature/".to_vec(), b"topic/".to_vec()));
        o.no_data = true;
    });
    let (_c1, head_after, _e1) = run_git(&repo, &["symbolic-ref", "HEAD"]);
    assert_eq!(head_after.trim(), headref);
}

#[test]
fn multi_branch_prefix_rename_maps_all_and_preserves_others() {
    let repo = init_repo();
    let (_c0, headref, _e0) = run_git(&repo, &["symbolic-ref", "HEAD"]);
    let headref = headref.trim().to_string();
    let def_short = headref
        .strip_prefix("refs/heads/")
        .unwrap_or(&headref)
        .to_string();

    assert_eq!(run_git(&repo, &["checkout", "-q", "-b", "features/foo"]).0, 0);
    write_file(&repo, "f-foo.txt", "foo");
    run_git(&repo, &["add", "."]).0;
    run_git(&repo, &["commit", "-q", "-m", "feat foo"]).0;
    assert_eq!(run_git(&repo, &["checkout", "-q", &def_short]).0, 0);
    assert_eq!(run_git(&repo, &["checkout", "-q", "-b", "features/bar"]).0, 0);
    write_file(&repo, "f-bar.txt", "bar");
    run_git(&repo, &["add", "."]).0;
    run_git(&repo, &["commit", "-q", "-m", "feat bar"]).0;
    assert_eq!(run_git(&repo, &["checkout", "-q", &def_short]).0, 0);
    assert_eq!(run_git(&repo, &["checkout", "-q", "-b", "misc/baz"]).0, 0);
    write_file(&repo, "baz.txt", "baz");
    run_git(&repo, &["add", "."]).0;
    run_git(&repo, &["commit", "-q", "-m", "misc baz"]).0;

    let (_c, _o, _e) = run_tool(&repo, |o| {
        o.branch_rename = Some((b"features/".to_vec(), b"topics/".to_vec()));
        o.no_data = true;
    });

    let (_c1, out_topics_foo, _e1) =
        run_git(&repo, &["show-ref", "--verify", "refs/heads/topics/foo"]);
    assert!(!out_topics_foo.is_empty());
    let (_c2, out_topics_bar, _e2) =
        run_git(&repo, &["show-ref", "--verify", "refs/heads/topics/bar"]);
    assert!(!out_topics_bar.is_empty());
    let (_c3, out_features_foo, _e3) =
        run_git(&repo, &["show-ref", "--verify", "refs/heads/features/foo"]);
    assert!(out_features_foo.is_empty());
    let (_c4, out_features_bar, _e4) =
        run_git(&repo, &["show-ref", "--verify", "refs/heads/features/bar"]);
    assert!(out_features_bar.is_empty());
    let (_c5, out_misc_baz, _e5) =
        run_git(&repo, &["show-ref", "--verify", "refs/heads/misc/baz"]);
    assert!(!out_misc_baz.is_empty());
}

#[test]
fn multi_branch_prefix_rename_maps_head_from_deleted_branch() {
    let repo = init_repo();
    let (_c0, headref, _e0) = run_git(&repo, &["symbolic-ref", "HEAD"]);
    let headref = headref.trim().to_string();
    let def_short = headref
        .strip_prefix("refs/heads/")
        .unwrap_or(&headref)
        .to_string();

    assert_eq!(run_git(&repo, &["checkout", "-q", "-b", "features/foo"]).0, 0);
    write_file(&repo, "f-foo.txt", "foo");
    run_git(&repo, &["add", "."]).0;
    run_git(&repo, &["commit", "-q", "-m", "feat foo"]).0;
    assert_eq!(run_git(&repo, &["checkout", "-q", &def_short]).0, 0);
    assert_eq!(run_git(&repo, &["checkout", "-q", "-b", "features/bar"]).0, 0);
    write_file(&repo, "f-bar.txt", "bar");
    run_git(&repo, &["add", "."]).0;
    run_git(&repo, &["commit", "-q", "-m", "feat bar"]).0;

    let (_c_h, head_before, _e_h) = run_git(&repo, &["symbolic-ref", "HEAD"]);
    assert_eq!(head_before.trim(), "refs/heads/features/bar");

    let (_c, _o, _e) = run_tool(&repo, |o| {
        o.branch_rename = Some((b"features/".to_vec(), b"topics/".to_vec()));
        o.no_data = true;
    });

    let (_c1, head_after, _e1) = run_git(&repo, &["symbolic-ref", "HEAD"]);
    assert_eq!(head_after.trim(), "refs/heads/topics/bar");
}
