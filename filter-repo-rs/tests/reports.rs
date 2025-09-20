use std::fs::File;
use std::io::Read;

mod common;
use common::*;

#[test]
fn strip_report_written() {
    let repo = init_repo();
    write_file(&repo, "small.txt", "x");
    let big_data = vec![b'A'; 10_000];
    let mut f = File::create(repo.join("big.bin")).unwrap();
    use std::io::Write as _;
    f.write_all(&big_data).unwrap();
    f.flush().unwrap();
    drop(f);
    run_git(&repo, &["add", "."]).0;
    assert_eq!(run_git(&repo, &["commit", "-q", "-m", "add files"]).0, 0);
    let (_c, _o, _e) = run_tool(&repo, |o| {
        o.max_blob_size = Some(1024);
        o.write_report = true;
    });
    let (_c2, tree_after, _e2) = run_git(&repo, &["ls-tree", "-r", "--name-only", "HEAD"]);
    assert!(!tree_after.contains("big.bin"));
    assert!(tree_after.contains("small.txt"));
    let report = repo.join(".git").join("filter-repo").join("report.txt");
    assert!(report.exists());
    let mut s = String::new();
    File::open(&report).unwrap().read_to_string(&mut s).unwrap();
    assert!(s.contains("Blobs stripped by size"));
}

#[test]
fn strip_ids_report_written() {
    let repo = init_repo();
    write_file(&repo, "secret.bin", "topsecret\n");
    run_git(&repo, &["add", "."]).0;
    assert_eq!(run_git(&repo, &["commit", "-q", "-m", "add secret.bin"]).0, 0);
    let (_c0, blob_id, _e0) = run_git(&repo, &["rev-parse", "HEAD:secret.bin"]);
    let sha = blob_id.trim();
    let shalist = repo.join("strip-sha.txt");
    std::fs::write(&shalist, format!("{}\n", sha)).unwrap();
    let (_c, _o, _e) = run_tool(&repo, |o| {
        o.strip_blobs_with_ids = Some(shalist.clone());
        o.write_report = true;
    });
    let (_c1, tree, _e1) = run_git(&repo, &["ls-tree", "-r", "--name-only", "HEAD"]);
    assert!(!tree.contains("secret.bin"));
    let report = repo.join(".git").join("filter-repo").join("report.txt");
    let mut s = String::new();
    File::open(&report).unwrap().read_to_string(&mut s).unwrap();
    assert!(s.contains("Blobs stripped by SHA:"));
    assert!(s.contains("secret.bin"));
}

