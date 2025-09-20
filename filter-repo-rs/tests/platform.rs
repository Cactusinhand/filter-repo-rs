mod common;
use common::*;

#[test]
fn cross_platform_windows_path_handling() {
    let repo = init_repo();
    let windows_paths = vec![
        "file_with_backslash/path/test.txt",
        "file/with/mixed/separators.txt",
        "relative/path/file.txt",
        "./hidden/file.txt",
        "../parent/file.txt",
    ];
    for (i, path_str) in windows_paths.iter().enumerate() {
        let content = format!("Windows path test file {} content", i);
        if let Some(parent) = std::path::Path::new(path_str).parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(repo.join(parent))
                    .expect("Failed to create parent directory");
            }
        }
        std::fs::write(repo.join(path_str), content)
            .expect("Failed to write test file for Windows path handling");
        run_git(&repo, &["add", path_str]);
    }
    run_git(&repo, &["commit", "-m", "Windows path compatibility test"]);
    run_tool_expect_success(&repo, |o| {
        o.max_blob_size = Some(50);
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
    assert!(!files.is_empty());
}

#[test]
fn cross_platform_case_sensitivity_handling() {
    let repo = init_repo();
    let test_files = vec![
        "TestFile.txt",
        "testfile.txt",
        "TESTFILE.TXT",
        "TestFile.TXT",
        "subdir/File.txt",
        "subdir/file.txt",
    ];
    for file_path in test_files {
        if let Some(parent) = std::path::Path::new(file_path).parent() {
            std::fs::create_dir_all(repo.join(parent)).unwrap();
        }
        std::fs::write(repo.join(file_path), format!("Content for {}", file_path)).unwrap();
        run_git(&repo, &["add", file_path]);
    }
    run_git(&repo, &["commit", "-m", "Case sensitivity test"]);
    run_tool_expect_success(&repo, |o| {
        o.max_blob_size = Some(20);
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
    assert!(!files.is_empty());
}

#[test]
fn cross_platform_special_characters_in_paths() {
    let repo = init_repo();
    let special_paths = vec![
        "file with spaces.txt",
        "file-with-dashes.txt",
        "file_with_underscores.txt",
        "file.with.dots.txt",
        "file123.txt",
    ];
    let mut files_created = 0;
    let mut successfully_created = Vec::new();
    for (_i, path_str) in special_paths.iter().enumerate() {
        // Keep content small so it stays below the size filter threshold
        let content = "ok";
        match std::fs::write(repo.join(path_str), content) {
            Ok(_) => {
                let (code, _output, _error) = run_git(&repo, &["add", path_str]);
                if code == 0 {
                    files_created += 1;
                    successfully_created.push(path_str.to_string());
                }
            }
            Err(_e) => continue,
        }
    }
    if files_created > 0 {
        assert_eq!(run_git(&repo, &["commit", "-m", "Special characters in paths"]).0, 0);
        run_tool_expect_success(&repo, |o| {
            o.max_blob_size = Some(30);
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
        let files: Vec<&str> = tree
            .lines()
            .map(|l| l.trim())
            .filter(|l| !l.is_empty())
            .collect();
        assert!(!files.is_empty());
        // Ensure at least one of our created paths appears verbatim in the tree
        let mut found = false;
        for created in &successfully_created {
            if files.iter().any(|f| f == &created.as_str()) {
                found = true;
                break;
            }
        }
        assert!(
            found,
            "expected at least one of {:?} in {:?}",
            successfully_created, files
        );
    }
}

#[test]
fn cross_platform_long_file_names() {
    let repo = init_repo();
    let long_paths = vec![
        "long_file_name_".to_string() + &"a".repeat(100) + ".txt",
        "nested/long_file_name_".to_string() + &"b".repeat(80) + ".txt",
        "another/nested/path/".to_string() + &"c".repeat(60) + ".md",
    ];
    for (i, path_str) in long_paths.iter().enumerate() {
        let normalized_path = path_str.replace('\\', "/");
        let content = format!("Long file name test {} content", i);
        if let Some(parent) = std::path::Path::new(&normalized_path).parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(repo.join(parent)).unwrap();
            }
        }
        std::fs::write(repo.join(&normalized_path), content).unwrap();
        run_git(&repo, &["add", &normalized_path]);
    }
    run_git(&repo, &["commit", "-m", "Long file names test"]);
    run_tool_expect_success(&repo, |o| {
        o.max_blob_size = Some(30);
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
    assert!(!files.is_empty());
    for file in files {
        if file.contains("long_file_name") {
            assert!(file.len() > 10);
        }
    }
}

#[test]
fn cross_platform_unicode_normalization() {
    let repo = init_repo();
    // NFC vs NFD for 'é'
    let nfc = "é"; // U+00E9
    let nfd = "e\u{0301}"; // e + COMBINING ACUTE ACCENT
    let p1 = format!("nfc_{}.txt", nfc);
    let p2 = format!("nfd_{}.txt", nfd);
    let _ = std::fs::write(repo.join(&p1), "a");
    let _ = std::fs::write(repo.join(&p2), "b");
    run_git(&repo, &["add", "."]);
    run_git(&repo, &["commit", "-m", "unicode normalization"]);
    run_tool_expect_success(&repo, |o| {
        o.max_blob_size = Some(10);
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
    assert!(tree.contains("nfc_") || tree.contains("nfd_"));
}

#[test]
fn cross_platform_line_endings() {
    let repo = init_repo();
    let crlf = "hello\r\nworld\r\n";
    let lf = "hello\nworld\n";
    std::fs::write(repo.join("crlf.txt"), crlf).unwrap();
    std::fs::write(repo.join("lf.txt"), lf).unwrap();
    run_git(&repo, &["add", "."]);
    run_git(&repo, &["commit", "-m", "line endings"]);
    run_tool_expect_success(&repo, |o| {
        o.max_blob_size = Some(1000);
    });
    let (_c2, tree, _e2) = run_git(&repo, &["ls-tree", "-r", "--name-only", "HEAD"]);
    assert!(tree.contains("crlf.txt") && tree.contains("lf.txt"));
}

#[test]
fn cross_platform_file_permissions() {
    let repo = init_repo();
    std::fs::write(repo.join("perm.txt"), "x").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(repo.join("perm.txt"))
            .unwrap()
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(repo.join("perm.txt"), perms).unwrap();
    }
    run_git(&repo, &["add", "."]);
    run_git(&repo, &["commit", "-m", "perms"]);
    run_tool_expect_success(&repo, |o| {
        o.max_blob_size = Some(1000);
    });
}
