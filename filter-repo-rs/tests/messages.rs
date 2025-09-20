mod common;
use common::*;

#[test]
fn replace_message_edits_commit_and_tag_messages() {
    let repo = init_repo();
    write_file(&repo, "src/a.txt", "x");
    run_git(&repo, &["add", "."]).0;
    assert_eq!(
        run_git(&repo, &["commit", "-q", "-m", "commit with FOO token"]).0,
        0
    );
    assert_eq!(
        run_git(&repo, &["tag", "-a", "-m", "tag msg FOO", "v2.0"]).0,
        0
    );
    let repl = repo.join("repl.txt");
    std::fs::write(&repl, "FOO==>BAR\n").unwrap();
    run_tool_expect_success(&repo, |o| {
        o.replace_message_file = Some(repl.clone());
        o.no_data = true;
    });
    let (_c1, msg, _e1) = run_git(&repo, &["log", "-1", "--format=%B"]);
    assert!(msg.contains("BAR"));
    assert!(!msg.contains("FOO"));
    let (_c2, tag_oid, _e2) = run_git(&repo, &["rev-parse", "refs/tags/v2.0"]);
    let tag_oid = tag_oid.trim();
    let (_c3, tag_obj, _e3) = run_git(&repo, &["cat-file", "-p", tag_oid]);
    assert!(tag_obj.contains("BAR"));
}

#[test]
fn second_run_rewrites_short_hashes_in_messages() {
    let repo = init_repo();
    write_file(&repo, "keep/data.txt", "first keep\n");
    write_file(&repo, "drop/data.txt", "first drop\n");
    assert_eq!(run_git(&repo, &["add", "."]).0, 0);
    assert_eq!(run_git(&repo, &["commit", "-m", "seed data"]).0, 0);

    let (_c_old, commit1_full, _e_old) = run_git(&repo, &["rev-parse", "HEAD"]);
    let commit1_full = commit1_full.trim().to_string();
    let old_short = commit1_full[..7].to_string();

    write_file(
        &repo,
        "keep/data.txt",
        &format!("updated keep referencing {}\n", old_short),
    );
    assert_eq!(run_git(&repo, &["add", "."]).0, 0);
    assert_eq!(
        run_git(
            &repo,
            &["commit", "-m", &format!("mention old short {}", old_short)]
        )
        .0,
        0
    );

    assert_eq!(
        run_git(
            &repo,
            &[
                "tag",
                "-a",
                "-m",
                &format!("tag cites {}", old_short),
                "v-short"
            ]
        )
        .0,
        0
    );

    run_tool_expect_success(&repo, |o| {
        o.paths.push(b"keep".to_vec());
    });

    let commit_map_path = repo.join(".git").join("filter-repo").join("commit-map");
    let map_contents =
        std::fs::read_to_string(&commit_map_path).expect("read commit-map after first run");
    let mut new_full: Option<String> = None;
    for line in map_contents.lines() {
        let mut parts = line.split_whitespace();
        if let (Some(old), Some(new)) = (parts.next(), parts.next()) {
            if old == commit1_full {
                new_full = Some(new.to_string());
                break;
            }
        }
    }
    let new_full = new_full.expect("commit-map should contain first commit mapping");
    let new_short = new_full[..7].to_string();
    assert_ne!(new_short, old_short);

    run_tool_expect_success(&repo, |o| {
        o.paths.push(b"keep".to_vec());
    });

    let (_c_msg, msg, _e_msg) = run_git(&repo, &["log", "-1", "--format=%B"]);
    assert!(msg.contains(&new_short));
    assert!(!msg.contains(&old_short));

    let (_c_tag, tag_obj, _e_tag) = run_git(&repo, &["cat-file", "-p", "refs/tags/v-short"]);
    assert!(tag_obj.contains(&new_short));
    assert!(!tag_obj.contains(&old_short));
}
