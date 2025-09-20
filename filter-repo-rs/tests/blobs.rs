mod common;
use common::*;

#[test]
fn max_blob_size_drops_large_blobs() {
    let repo = init_repo();
    let big = vec![b'A'; 4096];
    let small = vec![b'B'; 10];
    std::fs::write(repo.join("big.bin"), &big).unwrap();
    std::fs::write(repo.join("small.bin"), &small).unwrap();
    run_git(&repo, &["add", "."]).0;
    assert_eq!(run_git(&repo, &["commit", "-q", "-m", "add blobs"]).0, 0);
    let (_c, _o, _e) = run_tool(&repo, |o| {
        o.max_blob_size = Some(1024);
        o.no_data = false;
    });
    let (_c2, tree, _e2) = run_git(&repo, &["ls-tree", "-r", "--name-only", "HEAD"]);
    assert!(tree.contains("small.bin"));
    assert!(!tree.contains("big.bin"));
}

#[test]
fn max_blob_size_threshold_boundary() {
    let repo = init_repo();
    let exact_content = vec![b'X'; 1024];
    let just_over = vec![b'Y'; 1025];
    std::fs::write(repo.join("exact.txt"), &exact_content).unwrap();
    std::fs::write(repo.join("over.txt"), &just_over).unwrap();
    run_git(&repo, &["add", "."]).0;
    assert_eq!(
        run_git(&repo, &["commit", "-q", "-m", "add boundary test files"]).0,
        0
    );
    let (_c, _o, _e) = run_tool(&repo, |o| {
        o.max_blob_size = Some(1024);
    });
    let (_c2, tree, _e2) = run_git(&repo, &["ls-tree", "-r", "--name-only", "HEAD"]);
    assert!(tree.contains("exact.txt"));
    assert!(!tree.contains("over.txt"));
}

