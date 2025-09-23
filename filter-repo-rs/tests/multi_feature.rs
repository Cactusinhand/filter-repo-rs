use filter_repo_rs as fr;

mod common;
use common::*;

#[test]
fn multi_feature_blob_size_with_path_filtering() {
    let repo = init_repo();
    std::fs::create_dir_all(repo.join("keep")).unwrap();
    std::fs::create_dir_all(repo.join("filter")).unwrap();
    std::fs::write(repo.join("keep/large_file.txt"), "x".repeat(2000)).unwrap();
    std::fs::write(repo.join("filter/large_file.txt"), "x".repeat(2000)).unwrap();
    std::fs::write(repo.join("keep/small_file.txt"), "small").unwrap();
    std::fs::write(repo.join("filter/small_file.txt"), "small").unwrap();
    run_git(&repo, &["add", "."]);
    run_git(&repo, &["commit", "-m", "Add mixed size files"]);
    let mut opts = fr::Options::default();
    opts.max_blob_size = Some(1000);
    opts.paths = vec![b"keep/".to_vec()];
    opts.source = repo.clone();
    opts.target = repo.clone();
    opts.force = true; // Use --force to bypass sanity checks for unit tests
    let result = fr::run(&opts);
    assert!(result.is_ok());
    let (_c, tree, _e) = run_git(
        &repo,
        &[
            "-c",
            "core.quotepath=false",
            "ls-tree",
            "-r",
            "--name-only",
            "HEAD",
        ],
    );
    let files: Vec<&str> = tree.split_whitespace().collect();
    assert!(!files.iter().any(|f| f.starts_with("filter/")));
    assert!(files.contains(&"keep/small_file.txt"));
    assert!(!files.contains(&"keep/large_file.txt"));
    assert_eq!(files.len(), 1);
}

#[test]
fn multi_feature_blob_size_with_path_rename() {
    let repo = init_repo();
    std::fs::create_dir_all(repo.join("old_dir")).unwrap();
    std::fs::create_dir_all(repo.join("other_dir")).unwrap();
    std::fs::write(repo.join("old_dir/large_file.dat"), "x".repeat(1500)).unwrap();
    std::fs::write(repo.join("old_dir/small_file.txt"), "small content").unwrap();
    std::fs::write(repo.join("other_dir/medium_file.txt"), "x".repeat(800)).unwrap();
    run_git(&repo, &["add", "."]);
    run_git(
        &repo,
        &["commit", "-m", "Add files for size and rename test"],
    );
    let mut opts = fr::Options::default();
    opts.max_blob_size = Some(1000);
    opts.path_renames = vec![(b"old_dir/".to_vec(), b"new_dir/".to_vec())];
    opts.source = repo.clone();
    opts.target = repo.clone();
    opts.force = true; // Use --force to bypass sanity checks for unit tests
    let result = fr::run(&opts);
    assert!(result.is_ok());
    let (_c, tree, _e) = run_git(
        &repo,
        &[
            "-c",
            "core.quotepath=false",
            "ls-tree",
            "-r",
            "--name-only",
            "HEAD",
        ],
    );
    let files: Vec<&str> = tree.split_whitespace().collect();
    assert!(!files.iter().any(|f| f.starts_with("old_dir/")));
    assert!(files.contains(&"new_dir/small_file.txt"));
    assert!(!files.contains(&"new_dir/large_file.dat"));
    assert!(files.contains(&"other_dir/medium_file.txt"));
}

#[test]
fn multi_feature_blob_size_with_branch_rename() {
    let repo = init_repo();
    std::fs::write(repo.join("large_file.bin"), "x".repeat(2000)).unwrap();
    std::fs::write(repo.join("small_file.txt"), "small").unwrap();
    run_git(&repo, &["add", "."]);
    run_git(
        &repo,
        &["commit", "-m", "Add files for size and branch rename test"],
    );
    run_git(&repo, &["branch", "original-branch"]);
    let mut opts = fr::Options::default();
    opts.max_blob_size = Some(1000);
    opts.branch_rename = Some((b"original-".to_vec(), b"renamed-".to_vec()));
    opts.source = repo.clone();
    opts.target = repo.clone();
    opts.force = true; // Use --force to bypass sanity checks for unit tests
    opts.refs = vec!["--all".to_string()];
    let result = fr::run(&opts);
    assert!(result.is_ok());
    let (_c, branches, _e) = run_git(&repo, &["branch", "-l"]);
    assert!(branches.contains("renamed-branch"));
    assert!(!branches.contains("original-branch"));
    let (_c, tree, _e) = run_git(
        &repo,
        &[
            "-c",
            "core.quotepath=false",
            "ls-tree",
            "-r",
            "--name-only",
            "HEAD",
        ],
    );
    let files: Vec<&str> = tree.split_whitespace().collect();
    assert!(files.contains(&"small_file.txt"));
    assert!(!files.contains(&"large_file.bin"));
}

#[test]
fn multi_feature_blob_size_with_tag_rename() {
    let repo = init_repo();
    std::fs::write(repo.join("large_file.dat"), "x".repeat(3000)).unwrap();
    std::fs::write(repo.join("small_file.txt"), "small content").unwrap();
    run_git(&repo, &["add", "."]);
    run_git(
        &repo,
        &["commit", "-m", "Add files for size and tag rename test"],
    );
    run_git(&repo, &["tag", "original-tag", "HEAD"]);
    let mut opts = fr::Options::default();
    opts.max_blob_size = Some(1000);
    opts.tag_rename = Some((b"original-".to_vec(), b"renamed-".to_vec()));
    opts.source = repo.clone();
    opts.target = repo.clone();
    opts.force = true; // Use --force to bypass sanity checks for unit tests
    opts.refs = vec!["--all".to_string()];
    let result = fr::run(&opts);
    assert!(result.is_ok());
    let (_c, tags, _e) = run_git(&repo, &["tag", "-l"]);
    assert!(tags.contains("renamed-tag"));
    assert!(!tags.contains("original-tag"));
    let (_c, tree, _e) = run_git(
        &repo,
        &[
            "-c",
            "core.quotepath=false",
            "ls-tree",
            "-r",
            "--name-only",
            "HEAD",
        ],
    );
    let files: Vec<&str> = tree.split_whitespace().collect();
    assert!(files.contains(&"small_file.txt"));
    assert!(!files.contains(&"large_file.dat"));
}

#[test]
fn multi_feature_invert_paths_with_size_filtering() {
    let repo = init_repo();
    std::fs::create_dir_all(repo.join("keep")).unwrap();
    std::fs::create_dir_all(repo.join("drop")).unwrap();
    std::fs::write(repo.join("keep/small.txt"), "small").unwrap();
    std::fs::write(repo.join("drop/large.bin"), vec![0u8; 5000]).unwrap();
    run_git(&repo, &["add", "."]);
    run_git(&repo, &["commit", "-m", "seed"]);
    run_tool_expect_success(&repo, |o| {
        o.paths.push(b"drop/".to_vec());
        o.invert_paths = true;
        o.max_blob_size = Some(1000);
    });
    let (_c, tree, _e) = run_git(&repo, &["ls-tree", "-r", "--name-only", "HEAD"]);
    assert!(tree.contains("keep/small.txt"));
    assert!(!tree.contains("drop/large.bin"));
}

#[test]
fn multi_feature_complex_rename_chain() {
    let repo = init_repo();
    write_file(&repo, "src/a.txt", "x");
    run_git(&repo, &["add", "."]);
    run_git(&repo, &["commit", "-m", "add a"]);
    run_tool_expect_success(&repo, |o| {
        o.path_renames.push((b"src/".to_vec(), b"lib/".to_vec()));
        o.path_renames.push((b"lib/".to_vec(), b"app/".to_vec()));
    });
    let (_c, tree, _e) = run_git(&repo, &["ls-tree", "-r", "--name-only", "HEAD"]);
    assert!(tree.contains("app/a.txt"));
    assert!(!tree.contains("src/a.txt"));
}

#[test]
fn multi_feature_size_filter_with_special_paths() {
    let repo = init_repo();
    write_file(&repo, "file with spaces.txt", &"x".repeat(5000));
    write_file(&repo, "normal.txt", "ok");
    run_git(&repo, &["add", "."]);
    run_git(&repo, &["commit", "-m", "special names"]);
    run_tool_expect_success(&repo, |o| {
        o.max_blob_size = Some(1000);
    });
    let (_c2, tree, _e2) = run_git(
        &repo,
        &[
            "-c",
            "core.quotepath=false",
            "ls-tree",
            "-r",
            "--name-only",
            "HEAD",
        ],
    );
    assert!(tree.contains("normal.txt"));
    assert!(!tree.contains("file with spaces.txt"));
}

#[test]
fn multi_feature_empty_filtering_results() {
    let repo = init_repo();
    write_file(&repo, "big.bin", &"x".repeat(10_000));
    run_git(&repo, &["add", "."]);
    run_git(&repo, &["commit", "-m", "add big"]);
    run_tool_expect_success(&repo, |o| {
        o.max_blob_size = Some(1);
    });
    let (_c2, tree, _e2) = run_git(&repo, &["ls-tree", "-r", "--name-only", "HEAD"]);
    assert!(tree.lines().all(|l| l.trim().is_empty()));
}

#[test]
fn multi_feature_performance_with_multiple_filters() {
    let repo = init_repo();
    for i in 0..200 {
        let size = 100 + (i * 23) % 4000;
        std::fs::write(repo.join(format!("f{}.dat", i)), vec![b'Z'; size]).unwrap();
        run_git(&repo, &["add", &format!("f{}.dat", i)]);
    }
    run_git(&repo, &["commit", "-m", "pf dataset"]);
    let start = std::time::Instant::now();
    run_tool_expect_success(&repo, |o| {
        o.max_blob_size = Some(1500);
        o.paths = vec![b"f".to_vec()];
    });
    let dur = start.elapsed();
    assert!(dur > std::time::Duration::from_millis(0));
}
