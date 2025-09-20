use std::fs::File;
use std::io::Read;

mod common;
use common::*;

#[test]
fn writes_commit_map_and_ref_map() {
    let repo = init_repo();
    run_git(&repo, &["tag", "-a", "-m", "msg", "v3.0"]);
    run_tool_expect_success(&repo, |o| {
        o.no_data = true;
        o.tag_rename = Some((b"v".to_vec(), b"release-".to_vec()));
    });
    let debug = repo.join(".git").join("filter-repo");
    let commit_map = debug.join("commit-map");
    let ref_map = debug.join("ref-map");
    assert!(commit_map.exists());
    let mut s = String::new();
    File::open(&commit_map)
        .unwrap()
        .read_to_string(&mut s)
        .unwrap();
    assert!(!s.trim().is_empty());
    let mut r = String::new();
    File::open(&ref_map)
        .unwrap()
        .read_to_string(&mut r)
        .unwrap();
    assert!(r.contains("refs/tags/v3.0 refs/tags/release-3.0"));
}

#[test]
fn commit_map_records_pruned_commit_as_null() {
    let repo = init_repo();
    write_file(&repo, "keep/keep.txt", "keep one");
    assert_eq!(run_git(&repo, &["add", "."]).0, 0);
    assert_eq!(run_git(&repo, &["commit", "-m", "add keep file"]).0, 0);
    write_file(&repo, "drop/drop.txt", "drop me");
    assert_eq!(run_git(&repo, &["add", "."]).0, 0);
    assert_eq!(run_git(&repo, &["commit", "-m", "add drop file"]).0, 0);
    let (_code, drop_oid, _e) = run_git(&repo, &["rev-parse", "HEAD"]);
    let drop_oid = drop_oid.trim().to_string();
    run_tool_expect_success(&repo, |o| {
        o.paths.push(b"keep".to_vec());
    });
    let debug_dir = repo.join(".git").join("filter-repo");
    let commit_map = debug_dir.join("commit-map");
    assert!(commit_map.exists());
    let mut contents = String::new();
    File::open(&commit_map)
        .unwrap()
        .read_to_string(&mut contents)
        .unwrap();
    let null_oid = "0000000000000000000000000000000000000000";
    assert!(contents.contains(&format!("{} {}", drop_oid, null_oid)));
}
