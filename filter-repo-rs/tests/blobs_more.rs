mod common;
use common::*;

#[test]
fn max_blob_size_edge_cases() {
    let repo = init_repo();
    write_file(&repo, "empty.txt", "");
    write_file(&repo, "tiny.txt", "A");
    let threshold_content = vec![b'X'; 100];
    std::fs::write(repo.join("threshold.bin"), &threshold_content).unwrap();
    let over_content = vec![b'Y'; 101];
    std::fs::write(repo.join("over.bin"), &over_content).unwrap();
    let large_content = vec![b'Z'; 10000];
    std::fs::write(repo.join("large.bin"), &large_content).unwrap();
    run_git(&repo, &["add", "."]);
    run_git(&repo, &["commit", "-m", "add edge case files"]);
    let (_c, _o, _e) = run_tool(&repo, |o| { o.max_blob_size = Some(100); });
    let (_c2, tree, _e2) = run_git(&repo, &["ls-tree", "-r", "--name-only", "HEAD"]);
    assert!(tree.contains("empty.txt"));
    assert!(tree.contains("tiny.txt"));
    assert!(tree.contains("threshold.bin"));
    assert!(!tree.contains("over.bin"));
    assert!(!tree.contains("large.bin"));
}

#[test]
fn max_blob_size_with_path_filtering() {
    let repo = init_repo();
    std::fs::create_dir_all(repo.join("keep")).unwrap();
    std::fs::create_dir_all(repo.join("drop")).unwrap();
    let large_content = vec![b'A'; 2000];
    std::fs::write(repo.join("keep/large.bin"), &large_content).unwrap();
    std::fs::write(repo.join("drop/large.bin"), &large_content).unwrap();
    std::fs::write(repo.join("keep/small.txt"), "small content").unwrap();
    std::fs::write(repo.join("drop/small.txt"), "small content").unwrap();
    run_git(&repo, &["add", "."]);
    run_git(&repo, &["commit", "-m", "add files in different directories"]);
    let (_c, _o, _e) = run_tool(&repo, |o| { o.max_blob_size = Some(1000); o.paths.push(b"keep/".to_vec()); });
    let (_c2, tree, _e2) = run_git(&repo, &["ls-tree", "-r", "--name-only", "HEAD"]);
    assert!(tree.contains("keep/small.txt"));
    assert!(!tree.contains("drop/"));
    assert!(!tree.contains("keep/large.bin"));
}

#[test]
fn max_blob_size_with_strip_blobs_by_sha() {
    let repo = init_repo();
    let content1 = "test content 1";
    let content2 = "test content 2";
    std::fs::write(repo.join("file1.txt"), content1).unwrap();
    std::fs::write(repo.join("file2.txt"), content2).unwrap();
    run_git(&repo, &["add", "."]);
    run_git(&repo, &["commit", "-m", "add test files"]);
    let (_c1, sha1_output, _e1) = run_git(&repo, &["hash-object", "file1.txt"]);
    let (_c2, sha2_output, _e2) = run_git(&repo, &["hash-object", "file2.txt"]);
    let sha1 = sha1_output.trim();
    let sha2 = sha2_output.trim();
    let sha_list_content = format!("{}\n{}", sha1, sha2);
    std::fs::write(repo.join("sha_list.txt"), &sha_list_content).unwrap();
    run_git(&repo, &["add", "sha_list.txt"]);
    run_git(&repo, &["commit", "-m", "add sha list"]);
    let (_c, _o, _e) = run_tool(&repo, |o| { o.max_blob_size = Some(1000); o.strip_blobs_with_ids = Some(repo.join("sha_list.txt")); });
    let (_c2, tree, _e2) = run_git(&repo, &["ls-tree", "-r", "--name-only", "HEAD"]);
    assert!(!tree.contains("file1.txt"));
    assert!(!tree.contains("file2.txt"));
}

#[test]
fn max_blob_size_empty_repository() {
    let repo = init_repo();
    let (_c, _o, _e) = run_tool(&repo, |o| { o.max_blob_size = Some(1000); });
    let (_c2, tree, _e2) = run_git(&repo, &["ls-tree", "-r", "--name-only", "HEAD"]);
    assert!(tree.contains("README.md"));
}

#[test]
fn max_blob_size_mixed_blob_types() {
    let repo = init_repo();
    write_file(&repo, "text.txt", &"a".repeat(1500));
    std::fs::write(repo.join("binary.bin"), vec![0u8; 1500]).unwrap();
    write_file(&repo, "utf8.txt", &"浣犲ソ".repeat(500));
    std::fs::write(repo.join("zeroes.bin"), vec![0u8; 500]).unwrap();
    run_git(&repo, &["add", "."]);
    run_git(&repo, &["commit", "-m", "add mixed content types"]);
    let (_c, _o, _e) = run_tool(&repo, |o| { o.max_blob_size = Some(1000); });
    let (_c2, tree, _e2) = run_git(&repo, &["ls-tree", "-r", "--name-only", "HEAD"]);
    assert!(tree.contains("zeroes.bin"));
    assert!(!tree.contains("text.txt"));
    assert!(!tree.contains("binary.bin"));
    assert!(!tree.contains("utf8.txt"));
}

#[test]
fn max_blob_size_batch_optimization_verification() {
    let repo = init_repo();
    for i in 0..100 {
        let content = format!("file content {}", i);
        write_file(&repo, &format!("file{}.txt", i), &content);
    }
    write_file(&repo, "large1.bin", &"a".repeat(2000));
    write_file(&repo, "large2.bin", &"b".repeat(3000));
    run_git(&repo, &["add", "."]);
    run_git(&repo, &["commit", "-m", "add many files for batch test"]);
    let (_c, _o, _e) = run_tool(&repo, |o| { o.max_blob_size = Some(1500); });
    let (_c2, tree, _e2) = run_git(&repo, &["ls-tree", "-r", "--name-only", "HEAD"]);
    for i in 0..100 { assert!(tree.contains(&format!("file{}.txt", i))); }
    assert!(!tree.contains("large1.bin"));
    assert!(!tree.contains("large2.bin"));
}

#[test]
fn max_blob_size_fallback_behavior() {
    let repo = tempfile::TempDir::new().unwrap();
    let repo_path = repo.path();
    let (c, _o, e) = run_git(&repo_path, &["init"]);
    assert_eq!(c, 0, "git init failed: {}", e);
    run_git(&repo_path, &["config", "user.name", "A U Thor"]).0;
    run_git(&repo_path, &["config", "user.email", "a.u.thor@example.com"]).0;
    write_file(&repo_path, "test.txt", "hello");
    run_git(&repo_path, &["add", "."]);
    run_git(&repo_path, &["commit", "-m", "add test file"]);
    let (_c, _o, _e) = run_tool(&repo_path, |o| { o.max_blob_size = Some(1000); });
    let (_c2, tree, _e2) = run_git(&repo_path, &["-c", "core.quotepath=false", "ls-tree", "-r", "--name-only", "HEAD"]);
    assert!(tree.contains("test.txt"));
}

#[test]
fn max_blob_size_no_git_objects() {
    let repo = tempfile::TempDir::new().unwrap();
    let repo_path = repo.path();
    let (c, _o, e) = run_git(repo_path, &["init"]);
    assert_eq!(c, 0, "git init failed: {}", e);
    run_git(repo_path, &["config", "user.name", "test"]).0;
    run_git(repo_path, &["config", "user.email", "test@example.com"]).0;
    run_git(repo_path, &["commit", "--allow-empty", "-q", "-m", "empty commit 1"]).0;
    run_git(repo_path, &["commit", "--allow-empty", "-q", "-m", "empty commit 2"]).0;
    let (_c, _o, _e) = run_tool(repo_path, |o| { o.max_blob_size = Some(1000); });
    let (_c2, tree, _e2) = run_git(repo_path, &["-c", "core.quotepath=false", "ls-tree", "-r", "--name-only", "HEAD"]);
    assert!(tree.is_empty());
}

#[test]
fn max_blob_size_corrupted_git_output() {
    let repo = init_repo();
    write_file(&repo, "test.txt", "test content");
    run_git(&repo, &["add", "."]);
    run_git(&repo, &["commit", "-m", "add test file"]);
    let (_c, _o, _e) = run_tool(&repo, |o| { o.max_blob_size = Some(5); });
    let (_c2, tree, _e2) = run_git(&repo, &["-c", "core.quotepath=false", "ls-tree", "-r", "--name-only", "HEAD"]);
    assert!(!tree.contains("test.txt"));
}

#[test]
fn max_blob_size_extreme_threshold_values() {
    let repo = init_repo();
    write_file(&repo, "tiny.txt", "x");
    write_file(&repo, "small.txt", &"x".repeat(100));
    write_file(&repo, "medium.txt", &"x".repeat(10000));
    write_file(&repo, "large.txt", &"x".repeat(100000));
    run_git(&repo, &["add", "."]);
    run_git(&repo, &["commit", "-m", "add various sized files"]);
    let (_c, _o, _e) = run_tool(&repo, |o| { o.max_blob_size = Some(1); });
    let (_c2, tree1, _e2) = run_git(&repo, &["-c", "core.quotepath=false", "ls-tree", "-r", "--name-only", "HEAD"]);
    assert!(tree1.contains("tiny.txt"));
    assert!(!tree1.contains("small.txt"));
    assert!(!tree1.contains("medium.txt"));
    assert!(!tree1.contains("large.txt"));
    let repo2 = init_repo();
    write_file(&repo2, "tiny.txt", "x");
    write_file(&repo2, "small.txt", &"x".repeat(100));
    write_file(&repo2, "medium.txt", &"x".repeat(10000));
    write_file(&repo2, "large.txt", &"x".repeat(100000));
    run_git(&repo2, &["add", "."]);
    run_git(&repo2, &["commit", "-m", "add various sized files"]);
    let (_c, _o, _e) = run_tool(&repo2, |o| { o.max_blob_size = Some(1000000); });
    let (_c2, tree2, _e2) = run_git(&repo2, &["-c", "core.quotepath=false", "ls-tree", "-r", "--name-only", "HEAD"]);
    assert!(tree2.contains("tiny.txt"));
    assert!(tree2.contains("small.txt"));
    assert!(tree2.contains("medium.txt"));
    assert!(tree2.contains("large.txt"));
}

#[test]
fn max_blob_size_precise_threshold_handling() {
    let repo = init_repo();
    std::fs::write(repo.join("exactly_100_bytes.txt"), b"a".repeat(100)).unwrap();
    std::fs::write(repo.join("exactly_101_bytes.txt"), b"b".repeat(101)).unwrap();
    std::fs::write(repo.join("just_under_100.txt"), b"c".repeat(99)).unwrap();
    std::fs::write(repo.join("just_over_100.txt"), b"d".repeat(101)).unwrap();
    run_git(&repo, &["add", "."]);
    run_git(&repo, &["commit", "-m", "add boundary test files"]);
    let (_c, _o, _e) = run_tool(&repo, |o| { o.max_blob_size = Some(100); });
    let (_c2, tree, _e2) = run_git(&repo, &["-c", "core.quotepath=false", "ls-tree", "-r", "--name-only", "HEAD"]);
    assert!(tree.contains("exactly_100_bytes.txt"));
    assert!(tree.contains("just_under_100.txt"));
    assert!(!tree.contains("exactly_101_bytes.txt"));
    assert!(!tree.contains("just_over_100.txt"));
}
