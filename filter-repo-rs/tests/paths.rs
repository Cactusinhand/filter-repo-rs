use filter_repo_rs as fr;
use regex::bytes::Regex;

mod common;
use common::*;

#[test]
fn path_filter_includes_only_prefix() {
    let repo = init_repo();
    write_file(&repo, "src/keep.txt", "k");
    write_file(&repo, "docs/drop.txt", "d");
    run_git(&repo, &["add", "."]).0;
    assert_eq!(run_git(&repo, &["commit", "-q", "-m", "add files"]).0, 0);
    let (_c, _o, _e) = run_tool(&repo, |o| {
        o.paths.push(b"src/".to_vec());
    });
    // Read back with quoting disabled for human-readable output
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
    assert!(
        tree.contains("src/keep.txt"),
        "expected to keep src/keep.txt, got: {}",
        tree
    );
    assert!(
        !tree.contains("docs/drop.txt"),
        "expected to drop docs/drop.txt, got: {}",
        tree
    );
}

#[test]
fn path_rename_applies_to_paths() {
    let repo = init_repo();
    write_file(&repo, "a/file.txt", "x");
    run_git(&repo, &["add", "."]).0;
    assert_eq!(
        run_git(&repo, &["commit", "-q", "-m", "add a/file.txt"]).0,
        0
    );
    let (_c, _o, _e) = run_tool(&repo, |o| {
        o.path_renames.push((b"a/".to_vec(), b"x/".to_vec()));
    });
    let (_c2, tree, _e2) = run_git(&repo, &["ls-tree", "-r", "--name-only", "HEAD"]);
    assert!(
        tree.contains("x/file.txt"),
        "expected path renamed to x/file.txt, got: {}",
        tree
    );
    assert!(
        !tree.contains("a/file.txt"),
        "expected old path a/file.txt removed, got: {}",
        tree
    );
}

#[test]
fn path_glob_selects_md_under_src() {
    let repo = init_repo();
    write_file(&repo, "src/a.md", "m");
    write_file(&repo, "src/a.txt", "t");
    write_file(&repo, "src/deep/b.md", "m");
    write_file(&repo, "docs/x.md", "m");
    run_git(&repo, &["add", "."]).0;
    assert_eq!(
        run_git(&repo, &["commit", "-q", "-m", "add various files"]).0,
        0
    );
    let (_c, _o, _e) = run_tool(&repo, |o| {
        o.path_globs.push(b"src/**/*.md".to_vec());
    });
    let (_c2, tree, _e2) = run_git(&repo, &["ls-tree", "-r", "--name-only", "HEAD"]);
    assert!(
        tree.contains("src/a.md"),
        "expected to keep src/a.md, got: {}",
        tree
    );
    assert!(
        tree.contains("src/deep/b.md"),
        "expected to keep src/deep/b.md, got: {}",
        tree
    );
    assert!(
        !tree.contains("src/a.txt"),
        "expected to drop src/a.txt, got: {}",
        tree
    );
    assert!(
        !tree.contains("docs/x.md"),
        "expected to drop docs/x.md, got: {}",
        tree
    );
}

#[test]
fn path_filter_and_rename_updates_commit_and_ref_maps() {
    let repo = init_repo();

    // Create a branch that will be renamed and populate src/ with content to keep.
    assert_eq!(run_git(&repo, &["checkout", "-b", "feature/topic"]).0, 0);
    write_file(&repo, "src/lib.rs", "fn main() { println!(\"hi\"); }\n");
    run_git(&repo, &["add", "."]).0;
    assert_eq!(
        run_git(&repo, &["commit", "-q", "-m", "add src content"]).0,
        0
    );
    assert_eq!(
        run_git(&repo, &["tag", "-a", "-m", "cut first release", "v1"]).0,
        0
    );

    let (_c_old, old_head, _e_old) = run_git(&repo, &["rev-parse", "HEAD"]);
    let old_head = old_head.trim().to_string();

    let (code, _o, _e) = run_tool(&repo, |o| {
        o.paths.push(b"src/".to_vec());
        o.path_renames
            .push((b"src/".to_vec(), b"app/".to_vec()));
        o.branch_rename = Some((b"feature/".to_vec(), b"topics/".to_vec()));
        o.tag_rename = Some((b"v".to_vec(), b"release-".to_vec()));
    });
    assert_eq!(code, 0, "filter-repo-rs run should succeed");

    let (_c_new, new_head, _e_new) = run_git(&repo, &["rev-parse", "HEAD"]);
    let new_head = new_head.trim().to_string();

    // Only the renamed path should remain in the tree.
    let (_c_tree, tree, _e_tree) = run_git(
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
    assert!(tree.contains("app/lib.rs"), "expected renamed path in tree: {}", tree);
    assert!(
        !tree.contains("src/lib.rs"),
        "expected original path filtered out: {}",
        tree
    );

    // commit-map should capture the old -> new HEAD mapping.
    let commit_map = repo
        .join(".git")
        .join("filter-repo")
        .join("commit-map");
    let cm = std::fs::read_to_string(&commit_map).unwrap();
    assert!(
        cm.contains(&format!("{} {}", old_head, new_head)),
        "commit-map missing HEAD mapping; contents: {}",
        cm
    );

    // ref-map should record branch and tag renames.
    let ref_map = repo.join(".git").join("filter-repo").join("ref-map");
    let rm = std::fs::read_to_string(&ref_map).unwrap();
    assert!(
        rm.contains("refs/heads/feature/topic refs/heads/topics/topic"),
        "ref-map missing branch rename: {}",
        rm
    );
    assert!(
        rm.contains("refs/tags/v1 refs/tags/release-1"),
        "ref-map missing tag rename: {}",
        rm
    );

    // Verify final refs reflect the ref-map entries.
    let (_c_branch, branch_out, _e_branch) =
        run_git(&repo, &["show-ref", "--verify", "refs/heads/topics/topic"]);
    assert!(
        !branch_out.is_empty(),
        "expected renamed branch to exist: {}",
        branch_out
    );
    let (_c_branch_old, branch_old_out, _e_branch_old) =
        run_git(&repo, &["show-ref", "--verify", "refs/heads/feature/topic"]);
    assert!(
        branch_old_out.is_empty(),
        "expected original branch to be removed: {}",
        branch_old_out
    );

    let (_c_tag, tag_out, _e_tag) =
        run_git(&repo, &["show-ref", "--verify", "refs/tags/release-1"]);
    assert!(
        !tag_out.is_empty(),
        "expected renamed tag to exist: {}",
        tag_out
    );
    let (_c_tag_old, tag_old_out, _e_tag_old) =
        run_git(&repo, &["show-ref", "--verify", "refs/tags/v1"]);
    assert!(
        tag_old_out.is_empty(),
        "expected original tag to be removed: {}",
        tag_old_out
    );

    // HEAD should now point at the renamed branch.
    let (_c_head, head_ref, _e_head) = run_git(&repo, &["symbolic-ref", "HEAD"]);
    assert_eq!(
        head_ref.trim(),
        "refs/heads/topics/topic",
        "HEAD should track renamed branch"
    );
}

#[test]
fn invert_paths_drops_prefix() {
    let repo = init_repo();
    write_file(&repo, "src/keep.txt", "k");
    write_file(&repo, "drop/file.txt", "d");
    run_git(&repo, &["add", "."]).0;
    assert_eq!(
        run_git(&repo, &["commit", "-q", "-m", "prepare files"]).0,
        0
    );
    let (_c, _o, _e) = run_tool(&repo, |o| {
        o.paths.push(b"drop/".to_vec());
        o.invert_paths = true;
    });
    let (_c2, tree, _e2) = run_git(&repo, &["ls-tree", "-r", "--name-only", "HEAD"]);
    assert!(
        tree.contains("src/keep.txt"),
        "expected to keep src/keep.txt, got: {}",
        tree
    );
    assert!(
        !tree.contains("drop/file.txt"),
        "expected to drop drop/file.txt, got: {}",
        tree
    );
}

#[test]
fn quoted_paths_roundtrip_with_rename() {
    let repo = init_repo();
    // Force quoting in fast-export
    run_git(&repo, &["config", "core.quotepath", "true"]);
    // Create a filename with non-ASCII to trigger C-style quoting
    write_file(&repo, "src/ümlaut.txt", "u");
    run_git(&repo, &["add", "."]).0;
    assert_eq!(run_git(&repo, &["commit", "-q", "-m", "add umlaut"]).0, 0);
    // Move everything into X/ using to-subdirectory behavior; disable our quotepath override
    let (_c, _o, _e) = run_tool(&repo, |o| {
        o.quotepath = false; // do NOT pass -c core.quotepath=false
        o.path_renames.push((Vec::new(), b"X/".to_vec()));
        o.no_data = true;
    });
    let (_c2, tree, _e2) = run_git(&repo, &["ls-tree", "-r", "--name-only", "HEAD"]);
    // Normalize by dequoting any C-style quoted entries
    let mut found = false;
    for line in tree.lines() {
        let s = line.trim();
        if s.is_empty() {
            continue;
        }
        let norm = if s.starts_with('"') && s.ends_with('"') && s.len() >= 2 {
            let inner = &s.as_bytes()[1..s.as_bytes().len() - 1];
            String::from_utf8_lossy(&fr::dequote_c_style_bytes(inner)).to_string()
        } else {
            s.to_string()
        };
        if norm == "X/src/ümlaut.txt" {
            found = true;
            break;
        }
    }
    assert!(
        found,
        "expected renamed quoted path to roundtrip, got: {}",
        tree
    );
}

#[test]
fn path_regex_filters_and_respects_invert() {
    let repo = init_repo();
    write_file(&repo, "src/lib.rs", "fn main() {}\n");
    write_file(&repo, "README.md", "docs\n");
    write_file(&repo, "scripts/build.sh", "echo hi\n");
    run_git(&repo, &["add", "."]).0;
    assert_eq!(run_git(&repo, &["commit", "-q", "-m", "seed files"]).0, 0);
    let (_c, _o, _e) = run_tool(&repo, |o| {
        o.path_regexes.push(Regex::new(r".*\.rs$").unwrap());
    });
    let (_c_tree, tree, _e_tree) = run_git(
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
    assert!(tree.contains("src/lib.rs"));
    assert!(!tree.contains("README.md"));
    assert!(!tree.contains("scripts/build.sh"));

    // Inverted regex (drop *.md, keep others)
    let repo2 = init_repo();
    write_file(&repo2, "src/main.rs", "fn main() {}\n");
    write_file(&repo2, "docs/readme.md", "docs\n");
    write_file(&repo2, "notes/todo.txt", "todo\n");
    run_git(&repo2, &["add", "."]).0;
    assert_eq!(run_git(&repo2, &["commit", "-q", "-m", "seed docs"]).0, 0);
    let (_c2, _o2, _e2) = run_tool(&repo2, |o| {
        o.path_regexes.push(Regex::new(r".*\.md$").unwrap());
        o.invert_paths = true;
    });
    let (_c_tree2, tree2, _e_tree2) = run_git(
        &repo2,
        &[
            "-c",
            "core.quotepath=false",
            "ls-tree",
            "-r",
            "--name-only",
            "HEAD",
        ],
    );
    assert!(tree2.contains("src/main.rs"));
    assert!(tree2.contains("notes/todo.txt"));
    assert!(!tree2.contains("docs/readme.md"));
}

#[test]
fn windows_path_policy_sanitizes_or_preserves_bytes() {
    let cases = vec![
        (b"dir/inv:name?.txt ".as_ref(), b"dir/inv_name_.txt".as_ref()),
        (b"dir/trailing.dot.".as_ref(), b"dir/trailing.dot".as_ref()),
        (b"simple.txt".as_ref(), b"simple.txt".as_ref()),
    ];

    for (input, expected_windows) in cases {
        let sanitized = fr::pathutil::sanitize_invalid_windows_path_bytes(input);
        if cfg!(windows) {
            assert_eq!(sanitized, expected_windows.to_vec());
        } else {
            assert_eq!(sanitized, input.to_vec());
        }
    }
}
