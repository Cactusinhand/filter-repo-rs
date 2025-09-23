use filter_repo_rs as fr;

mod common;
use common::*;

#[test]
fn memory_management_path_filtering_memory() {
    let repo = init_repo();
    for dir in 0..10 {
        let dir_path = repo.join(format!("dir_{}", dir));
        std::fs::create_dir_all(&dir_path).unwrap();
        for file in 0..100 {
            let content = format!("content for dir {} file {}", dir, file);
            let file_path = dir_path.join(format!("file_{}.txt", file));
            std::fs::write(file_path, content).unwrap();
            run_git(&repo, &["add", &format!("dir_{}/file_{}.txt", dir, file)]);
        }
    }
    run_git(&repo, &["commit", "-m", "nested directories"]);

    let mut paths = Vec::new();
    for dir in 0..5 {
        paths.push(format!("dir_{}/", dir).into_bytes());
    }
    let opts = fr::Options {
        source: repo.clone(),
        target: repo.clone(),
        refs: vec!["--all".to_string()],
        max_blob_size: Some(1000),
        paths,
        force: true, // Use --force to bypass sanity checks for memory tests
        ..Default::default()
    };
    let _result = fr::run(&opts);
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
    assert!(files.len() > 0 && files.len() < 1000);
}

#[test]
fn memory_management_blob_size_precomputation_stress() {
    let repo = init_repo();
    for commit in 0..20 {
        for file in 0..50 {
            let size = 100 + (commit * file * 10) % 4000;
            let content = "x".repeat(size);
            let file_path = repo.join(format!("commit{}_file{}.txt", commit, file));
            std::fs::write(file_path, content).unwrap();
            run_git(
                &repo,
                &["add", &format!("commit{}_file{}.txt", commit, file)],
            );
        }
        run_git(&repo, &["commit", "-m", &format!("commit {}", commit)]);
    }
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
    let files: Vec<&str> = tree.split_whitespace().collect();
    assert!(files.len() > 0);
}

#[test]
fn memory_management_repeated_operations_same_repository() {
    let repo = init_repo();
    for i in 0..200 {
        let size = 100 + (i * 15) % 3000;
        let content = "x".repeat(size);
        std::fs::write(repo.join(format!("file_{}.txt", i)), content).unwrap();
        run_git(&repo, &["add", &format!("file_{}.txt", i)]);
    }
    run_git(&repo, &["commit", "-m", "initial commit"]);

    for iteration in 0..50 {
        let threshold = 500 + (iteration * 50) % 2500;
        let opts = fr::Options {
            source: repo.clone(),
            target: repo.clone(),
            refs: vec!["--all".to_string()],
            max_blob_size: Some(threshold),
            force: true, // Use --force to bypass sanity checks for memory tests
            ..Default::default()
        };
        let _result = fr::run(&opts);
        if iteration % 10 == 0 {
            let (_c, _tree, _e) = run_git(
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
        }
    }
}

#[test]
fn memory_management_edge_case_empty_repositories() {
    let repo = tempfile::TempDir::new().unwrap();
    let repo_path = repo.path();
    run_git(repo_path, &["init"]);
    run_git(repo_path, &["config", "user.name", "tester"]).0;
    run_git(repo_path, &["config", "user.email", "tester@example.com"]).0;
    run_git(repo_path, &["commit", "--allow-empty", "-m", "empty"]).0;
    common::run_tool(repo_path, |o| {
        o.max_blob_size = Some(1000);
    })
    .expect("filter-repo-rs run should succeed");
}

#[test]
fn memory_management_extreme_path_depth() {
    let repo = init_repo();
    let deep = (0..30)
        .map(|i| format!("d{}", i))
        .collect::<Vec<_>>()
        .join("/");
    let file = format!("{}/file.txt", deep);
    if let Some(parent) = std::path::Path::new(&file).parent() {
        std::fs::create_dir_all(repo.join(parent)).unwrap();
    }
    std::fs::write(repo.join(&file), "content").unwrap();
    run_git(&repo, &["add", "."]);
    run_git(&repo, &["commit", "-m", "deep paths"]);
    run_tool_expect_success(&repo, |o| {
        o.max_blob_size = Some(1000);
    });
}

#[test]
fn memory_management_unicode_path_heavy_load() {
    let repo = init_repo();
    let mut files_added = 0;
    for i in 0..200 {
        let name = format!("unicode_{}_é_測試.txt", i);
        if let Some(parent) = std::path::Path::new(&name).parent() {
            std::fs::create_dir_all(repo.join(parent))
                .expect("failed to create parent directory for unicode file");
        }
        std::fs::write(repo.join(&name), format!("payload {}", i))
            .expect("failed to write unicode file");
        if run_git(&repo, &["add", &name]).0 == 0 {
            files_added += 1;
        }
    }

    if files_added == 0 {
        eprintln!(
            "Warning: could not add any unicode files. Skipping unicode path heavy load test."
        );
        return;
    }
    assert_eq!(
        run_git(&repo, &["commit", "-m", "unicode files"]).0,
        0,
        "failed to commit unicode files"
    );
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
    let files: Vec<&str> = tree.lines().filter(|l| !l.is_empty()).collect();
    assert!(
        files.iter().any(|f| f.contains("unicode_")),
        "unicode files should be processed and present in the tree"
    );
}
