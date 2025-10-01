use filter_repo_rs as fr;

mod common;
use common::*;

#[test]
fn unit_test_commit_message_processing() {
    let repo = init_repo();
    std::fs::write(repo.join("test.txt"), "test content").unwrap();
    run_git(&repo, &["add", "test.txt"]);
    run_git(&repo, &["commit", "-m", "Original commit message"]);
    let message_file = repo.join("message_replacements.txt");
    std::fs::write(&message_file, "Original==>Replacement").unwrap();
    let mut opts = fr::Options::default();
    opts.replace_message_file = Some(message_file);
    opts.source = repo.clone();
    opts.target = repo.clone();
    opts.force = true; // Use --force to bypass sanity checks for unit tests
    let result = fr::run(&opts);
    assert!(result.is_ok());
    let (_c, log, _e) = run_git(&repo, &["log", "--oneline", "-1"]);
    assert!(log.contains("Replacement"));
    assert!(!log.contains("Original"));
}

#[test]
fn unit_test_tag_processing() {
    let repo = init_repo();
    std::fs::write(repo.join("test.txt"), "test content").unwrap();
    run_git(&repo, &["add", "test.txt"]);
    run_git(&repo, &["commit", "-m", "Test commit for tags"]);
    run_git(&repo, &["tag", "lightweight-tag"]);
    run_git(
        &repo,
        &["tag", "-a", "annotated-tag", "-m", "Annotated tag message"],
    );
    let mut opts = fr::Options::default();
    opts.tag_rename = Some((b"lightweight-".to_vec(), b"renamed-lightweight-".to_vec()));
    opts.source = repo.clone();
    opts.target = repo.clone();
    opts.force = true; // Use --force to bypass sanity checks for unit tests
    opts.refs = vec!["--all".to_string()];
    let result = fr::run(&opts);
    assert!(result.is_ok());
    let (_c, tags, _e) = run_git(&repo, &["tag", "-l"]);
    let tags_list: Vec<&str> = tags.split('\n').collect();
    assert!(tags_list.contains(&"renamed-lightweight-tag"));
    assert!(!tags_list.contains(&"lightweight-tag"));
    assert!(tags_list.contains(&"annotated-tag"));
}

#[test]
fn annotated_tag_message_replacement() {
    let repo = init_repo();
    std::fs::write(repo.join("test.txt"), "test content").unwrap();
    run_git(&repo, &["add", "test.txt"]);
    run_git(&repo, &["commit", "-m", "Release with café and 🚀"]);
    // Create annotated tag with a message that should be rewritten
    run_git(&repo, &["tag", "-a", "v1.0", "-m", "Tag message café 🚀"]);

    // Replacement rules to affect both commits and tag annotations
    let rules = repo.join("message_rules_tags.txt");
    std::fs::write(&rules, "café==>CAFE\n🚀==>ROCKET\n").unwrap();

    let mut opts = fr::Options::default();
    opts.replace_message_file = Some(rules);
    opts.source = repo.clone();
    opts.target = repo.clone();
    opts.force = true; // Use --force to bypass sanity checks for unit tests
    opts.refs = vec!["--all".to_string()]; // include tags
    let result = fr::run(&opts);
    assert!(result.is_ok());

    // Verify annotated tag message is rewritten
    let (_c, tag_obj, _e) = run_git(&repo, &["cat-file", "-p", "refs/tags/v1.0"]);
    assert!(
        tag_obj.contains("CAFE"),
        "expected tag message to include CAFE"
    );
    assert!(
        !tag_obj.contains("café"),
        "unexpected original token in tag message"
    );
    assert!(
        tag_obj.contains("ROCKET"),
        "expected tag message to include ROCKET"
    );
    assert!(
        !tag_obj.contains("🚀"),
        "unexpected rocket emoji in tag message"
    );
}

#[test]
fn commit_message_short_hash_is_remapped() {
    let repo = init_repo();
    // Seed a commit that will be referenced by short hash and affected by filtering
    std::fs::create_dir_all(repo.join("keep")).unwrap();
    std::fs::create_dir_all(repo.join("drop")).unwrap();
    std::fs::write(repo.join("keep/ref.txt"), "reference").unwrap();
    // Avoid Windows reserved name 'aux' (AUX) by using a safe filename
    std::fs::write(repo.join("drop/aux_file.txt"), "aux").unwrap();
    run_git(&repo, &["add", "."]);
    run_git(&repo, &["commit", "-m", "seed both keep and drop"]);

    // Capture the seed commit oid and short form
    let (_c0, seed_full, _e0) = run_git(&repo, &["rev-parse", "HEAD"]);
    let seed_full = seed_full.trim().to_string();
    let old_short = &seed_full[0..7.min(seed_full.len())];

    // Create a new commit whose message references the seed commit's short id
    std::fs::write(repo.join("keep/ref.txt"), "reference updated").unwrap();
    run_git(&repo, &["add", "keep/ref.txt"]);
    let msg = format!("mentions old short {}", old_short);
    run_git(&repo, &["commit", "-m", &msg]);

    // First run: apply a path filter that changes commit IDs and produces commit-map
    run_tool_expect_success(&repo, |o| {
        o.paths.push(b"keep".to_vec());
    });

    // Extract the new id for the seed commit from commit-map
    let commit_map = repo.join(".git").join("filter-repo").join("commit-map");
    let map_data = std::fs::read_to_string(commit_map).expect("commit-map after first run");
    let mut new_seed_full: Option<String> = None;
    for line in map_data.lines() {
        let mut it = line.split_whitespace();
        if let (Some(old), Some(new_)) = (it.next(), it.next()) {
            if old.eq_ignore_ascii_case(&seed_full)
                && new_ != "0000000000000000000000000000000000000000"
            {
                new_seed_full = Some(new_.to_string());
                break;
            }
        }
    }
    let new_seed_full = new_seed_full.expect("seed commit mapping missing");
    let new_short = &new_seed_full[0..7.min(new_seed_full.len())];
    assert_ne!(new_short, old_short);

    // Second run: same filtering; commit_map from first run is used by short-hash mapper
    run_tool_expect_success(&repo, |o| {
        o.paths.push(b"keep".to_vec());
    });

    // Verify latest commit message contains remapped short hash and not the original
    let (_c1, latest_msg, _e1) = run_git(&repo, &["log", "-1", "--format=%B"]);
    assert!(
        latest_msg.contains(new_short),
        "expected remapped short hash in message"
    );
    assert!(
        !latest_msg.contains(old_short),
        "unexpected original short hash present in message"
    );
}

#[test]
fn tag_rename_and_message_rewrite_combined() {
    let repo = init_repo();

    // Create a base commit
    std::fs::write(repo.join("file.txt"), "content").unwrap();
    run_git(&repo, &["add", "file.txt"]);
    run_git(&repo, &["commit", "-m", "base commit"]);

    // Create tags: one annotated with a message to be rewritten, one lightweight
    assert_eq!(
        run_git(
            &repo,
            &["tag", "-a", "orig-ann", "-m", "Tag message café and 🚀"]
        )
        .0,
        0,
        "failed to create annotated tag"
    );
    assert_eq!(
        run_git(&repo, &["tag", "orig-light"]).0,
        0,
        "failed to create lightweight tag"
    );

    // Replacement rules for tag annotations
    let rules = repo.join("tag_rules.txt");
    std::fs::write(&rules, "café==>CAFE\n🚀==>ROCKET\n").unwrap();

    // Run with both tag rename and message replacement enabled
    let mut opts = fr::Options::default();
    opts.replace_message_file = Some(rules);
    opts.tag_rename = Some((b"orig-".to_vec(), b"renamed-".to_vec()));
    opts.source = repo.clone();
    opts.target = repo.clone();
    opts.force = true;
    opts.refs = vec!["--all".to_string()];
    let result = fr::run(&opts);
    assert!(result.is_ok());

    // Verify tag names were renamed
    let (_c_tags, tags, _e_tags) = run_git(&repo, &["tag", "-l"]);
    let tags_list: Vec<&str> = tags.lines().collect();
    assert!(tags_list.contains(&"renamed-ann"));
    assert!(tags_list.contains(&"renamed-light"));
    assert!(!tags_list.contains(&"orig-ann"));
    assert!(!tags_list.contains(&"orig-light"));

    // Verify annotated tag message content was rewritten
    let (_c_obj, tag_obj, _e_obj) = run_git(&repo, &["cat-file", "-p", "refs/tags/renamed-ann"]);
    assert!(tag_obj.contains("CAFE"), "expected rewritten 'café' token");
    assert!(
        tag_obj.contains("ROCKET"),
        "expected rewritten rocket token"
    );
    assert!(
        !tag_obj.contains("café"),
        "unexpected original 'café' token"
    );
    assert!(!tag_obj.contains("🚀"), "unexpected original rocket token");
}

#[test]
fn branch_rename_with_tag_message_rewrite() {
    let repo = init_repo();

    // Create two branches with the rename prefix and add commits
    assert_eq!(run_git(&repo, &["checkout", "-b", "original-feature"]).0, 0);
    write_file(&repo, "feat.txt", "feature");
    assert_eq!(run_git(&repo, &["add", "."]).0, 0);
    assert_eq!(run_git(&repo, &["commit", "-m", "feature work"]).0, 0);

    assert_eq!(run_git(&repo, &["checkout", "-b", "original-bugfix"]).0, 0);
    write_file(&repo, "fix.txt", "bugfix");
    assert_eq!(run_git(&repo, &["add", "."]).0, 0);
    assert_eq!(run_git(&repo, &["commit", "-m", "bugfix work"]).0, 0);

    // Create an annotated tag pointing at current HEAD with a message to be rewritten
    assert_eq!(
        run_git(
            &repo,
            &["tag", "-a", "v-branch", "-m", "Branch tag café 🚀"]
        )
        .0,
        0,
        "failed to create annotated tag"
    );

    // Replacement rules for tag annotations (and commit messages, if any)
    let rules = repo.join("msg_rules.txt");
    std::fs::write(&rules, "café==>CAFE\n🚀==>ROCKET\n").unwrap();

    // Run with branch rename and message replacement; include all refs (branches + tags)
    let mut opts = fr::Options::default();
    opts.replace_message_file = Some(rules);
    opts.branch_rename = Some((b"original-".to_vec(), b"renamed-".to_vec()));
    opts.source = repo.clone();
    opts.target = repo.clone();
    opts.force = true;
    opts.refs = vec!["--all".to_string()];
    let result = fr::run(&opts);
    assert!(result.is_ok());

    // Verify branches were renamed
    let (_c_br, branches, _e_br) = run_git(&repo, &["branch", "-l"]);
    let norm_names: Vec<String> = branches
        .lines()
        .map(|l| l.trim().trim_start_matches("* ").to_string())
        .collect();
    assert!(norm_names.iter().any(|s| s == "renamed-feature"));
    assert!(norm_names.iter().any(|s| s == "renamed-bugfix"));
    assert!(!norm_names.iter().any(|s| s == "original-feature"));
    assert!(!norm_names.iter().any(|s| s == "original-bugfix"));

    // Verify the tag still exists under the same name and its message was rewritten
    let (_c_tag_l, tags_list, _e_tag_l) = run_git(&repo, &["tag", "-l"]);
    assert!(tags_list.lines().any(|t| t.trim() == "v-branch"));
    let (_c_tag, tag_obj, _e_tag) = run_git(&repo, &["cat-file", "-p", "refs/tags/v-branch"]);
    assert!(tag_obj.contains("CAFE"));
    assert!(tag_obj.contains("ROCKET"));
    assert!(!tag_obj.contains("café"));
    assert!(!tag_obj.contains("🚀"));
}

#[test]
fn head_moves_on_branch_rename() {
    let repo = init_repo();

    // Create and switch to a branch that will be renamed
    assert_eq!(run_git(&repo, &["checkout", "-b", "original-topic"]).0, 0);
    write_file(&repo, "topic.txt", "topic work");
    assert_eq!(run_git(&repo, &["add", "."]).0, 0);
    assert_eq!(run_git(&repo, &["commit", "-m", "topic commit"]).0, 0);

    // Sanity: HEAD should point to original-topic
    let (_c0, head_before, _e0) = run_git(&repo, &["symbolic-ref", "-q", "HEAD"]);
    assert!(head_before.trim().ends_with("refs/heads/original-topic"));

    // Run pipeline with branch rename
    let mut opts = fr::Options::default();
    opts.branch_rename = Some((b"original-".to_vec(), b"renamed-".to_vec()));
    opts.source = repo.clone();
    opts.target = repo.clone();
    opts.force = true;
    opts.refs = vec!["--all".to_string()];
    let result = fr::run(&opts);
    assert!(result.is_ok());

    // Verify HEAD now points to the renamed branch
    let (_c1, head_after, _e1) = run_git(&repo, &["symbolic-ref", "-q", "HEAD"]);
    let head_after = head_after.trim().to_string();
    assert_eq!(head_after, "refs/heads/renamed-topic");

    // Branch list reflects rename
    let (_c2, branches, _e2) = run_git(&repo, &["branch", "-l"]);
    let list: Vec<String> = branches
        .lines()
        .map(|l| l.trim().trim_start_matches("* ").to_string())
        .collect();
    assert!(list.iter().any(|b| b == "renamed-topic"));
    assert!(!list.iter().any(|b| b == "original-topic"));
}

#[test]
fn unit_test_path_utilities() {
    use filter_repo_rs::pathutil;
    let unquoted = b"test\npath\tab";
    let dequoted = pathutil::dequote_c_style_bytes(unquoted);
    assert_eq!(dequoted, b"test\npath\tab");
    let unquoted = b"regular_path";
    let result = pathutil::dequote_c_style_bytes(unquoted);
    assert_eq!(result, unquoted);
    let empty = b"";
    let result = pathutil::dequote_c_style_bytes(empty);
    assert_eq!(result, empty);
}

#[test]
fn unit_test_git_utilities() {
    let repo = init_repo();
    std::fs::write(repo.join("test.txt"), "test").unwrap();
    run_git(&repo, &["add", "test.txt"]);
    run_git(&repo, &["commit", "-m", "Test commit"]);
    let (_c, head_ref_out, _e) = run_git(&repo, &["symbolic-ref", "HEAD"]);
    let head_ref = head_ref_out.trim();
    let (_c, output, _e) = run_git(&repo, &["show-ref", head_ref]);
    assert!(!output.is_empty());
}
